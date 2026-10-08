//! Discord Rich Presence: "Hosting The Sims 4 · 2 friends connected" on the
//! user's Discord profile while a session runs. Talks to the Discord app on
//! this PC over its local IPC socket (named pipe `discord-ipc-N` on Windows,
//! a Unix socket elsewhere); nothing goes over the network from here.
//!
//! The frontend decides what to show (`set_discord_presence`) and never sends
//! join codes, PINs or friends' names. A worker thread owns the connection:
//! it coalesces updates (Discord allows about 5 per 20 s), connects lazily,
//! and retries quietly when Discord isn't running or restarts.
use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// The SyncCrate application on Discord's developer portal; its name is what
/// profiles show after "Playing". Not a secret: every Rich Presence client ships it.
const CLIENT_ID: &str = "1557724028129771530";
const MIN_GAP: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(20);
/// How long to wait for READY or a SET_ACTIVITY reply before giving up on the pipe.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const WEBSITE: &str = "https://synccrate.app";
const ICON: &str = "https://synccrate.app/apple-touch-icon.png";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Presence {
    /// First line, e.g. "Hosting The Sims 4".
    pub details: String,
    /// Second line, e.g. "2 friends connected".
    pub state: String,
    /// Unix seconds; Discord shows "00:12 elapsed" from here.
    pub start: Option<u64>,
}

/// Discord rejects strings shorter than 2 or longer than 128 characters.
fn fit(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(128).collect();
    if s.chars().count() < 2 { format!("{s}  ") } else { s }
}

/// The SET_ACTIVITY payload for `presence` (None clears it).
pub fn activity_payload(presence: Option<&Presence>, pid: u32, nonce: u64) -> serde_json::Value {
    let activity = presence.map(|p| {
        let mut a = json!({
            "details": fit(&p.details),
            "state": fit(&p.state),
            "assets": { "large_image": ICON, "large_text": "SyncCrate" },
            "buttons": [{ "label": "Get SyncCrate", "url": WEBSITE }],
        });
        if let Some(start) = p.start {
            a["timestamps"] = json!({ "start": start });
        }
        a
    });
    json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid, "activity": activity }, "nonce": nonce.to_string() })
}

/// One IPC frame: opcode and length (little-endian u32s), then JSON.
pub fn frame(op: u32, body: &serde_json::Value) -> Vec<u8> {
    let data = body.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend(op.to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// Discord's opcode for "this connection is over" (bad client id, an
/// unsupported version), sent instead of READY or a reply.
const OP_CLOSE: u32 = 2;

/// One frame's opcode and body.
fn read_frame(p: &mut dyn Read) -> std::io::Result<(u32, Vec<u8>)> {
    let mut hdr = [0u8; 8];
    p.read_exact(&mut hdr)?;
    let op = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    // Replies are small; a huge length means a wrong peer, not Discord.
    if len > 64 * 1024 {
        return Err(std::io::Error::other("oversized frame"));
    }
    let mut body = vec![0u8; len];
    p.read_exact(&mut body)?;
    Ok((op, body))
}

/// Whether a reply means Discord took what was sent. Any frame used to count,
/// so a CLOSE on the handshake or an `"evt": "ERROR"` reply to SET_ACTIVITY
/// was taken as shown and never retried. A refusal is `ConnectionRefused`,
/// which the worker answers with the slow retry.
fn check_reply(op: u32, body: &[u8]) -> std::io::Result<()> {
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let message = json.get("message").or_else(|| json.pointer("/data/message")).and_then(|m| m.as_str()).unwrap_or("no reason given");
    let refused = |what: &str| std::io::Error::new(std::io::ErrorKind::ConnectionRefused, format!("{what}: {message}"));
    if op == OP_CLOSE {
        return Err(refused("Discord closed the connection"));
    }
    if json.get("evt").and_then(|e| e.as_str()) == Some("ERROR") {
        return Err(refused("Discord refused the request"));
    }
    Ok(())
}

type Reader = Box<dyn Read + Send>;
type Writer = Box<dyn Write + Send>;

/// An open IPC connection whose reads happen on a helper thread, so a Discord
/// that accepts the pipe but never answers can't stall the worker (and with it
/// the clear when hosting stops). The Windows pipe is a plain blocking handle
/// with no read timeout, which is how that hang happened.
struct Conn {
    writer: Writer,
    /// One message per frame the worker expects next.
    want: mpsc::Sender<()>,
    frames: mpsc::Receiver<std::io::Result<()>>,
    /// True until the reader thread has ended.
    reader_alive: Arc<AtomicBool>,
}

/// Clears the flag however the reader thread ends.
struct AliveGuard(Arc<AtomicBool>);
impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Conn {
    fn start(mut reader: Reader, writer: Writer) -> std::io::Result<Conn> {
        let (want, want_rx) = mpsc::channel::<()>();
        let (frames_tx, frames) = mpsc::channel();
        let reader_alive = Arc::new(AtomicBool::new(true));
        let guard = AliveGuard(reader_alive.clone());
        // Reads only on request: a synchronous Windows pipe serialises I/O on
        // the handle, so an idle read left pending would block the next write.
        std::thread::Builder::new().name("discord-ipc-read".into()).spawn(move || {
            let _guard = guard;
            while want_rx.recv().is_ok() {
                // A refusal ends the reader too: after a CLOSE nothing more comes.
                let r = read_frame(&mut *reader).and_then(|(op, body)| check_reply(op, &body));
                let failed = r.is_err();
                if frames_tx.send(r).is_err() || failed {
                    return;
                }
            }
        })?;
        Ok(Conn { writer, want, frames, reader_alive })
    }

    /// Sends `bytes` and waits up to `timeout` for Discord's reply frame. After a
    /// timeout the connection must be dropped: the reader may still be blocked
    /// and only ends once Discord answers or the pipe closes.
    fn request(&mut self, bytes: &[u8], timeout: Duration) -> std::io::Result<()> {
        // Write before asking for the read, for the same serialisation reason.
        self.writer.write_all(bytes)?;
        self.want.send(()).map_err(|_| std::io::Error::other("Discord reader stopped"))?;
        match self.frames.recv_timeout(timeout) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Discord didn't answer")),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::other("Discord reader stopped")),
        }
    }
}

#[cfg(windows)]
fn open_socket(i: u8) -> std::io::Result<(Reader, Writer)> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}"))?;
    Ok((Box::new(f.try_clone()?), Box::new(f)))
}

#[cfg(unix)]
fn open_socket(i: u8) -> std::io::Result<(Reader, Writer)> {
    let mut last = std::io::Error::other("no Discord socket");
    let bases: Vec<String> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|v| std::env::var(v).ok()).chain(["/tmp".to_string()]).collect();
    for base in bases {
        // Plain, Flatpak and Snap Discord.
        for sub in ["", "app/com.discordapp.Discord/", "snap.discord/"] {
            match std::os::unix::net::UnixStream::connect(format!("{base}/{sub}discord-ipc-{i}")) {
                Ok(s) => {
                    let _ = s.set_read_timeout(Some(REPLY_TIMEOUT));
                    return Ok((Box::new(s.try_clone()?), Box::new(s)));
                }
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

/// Whether the reader of an earlier connection is still running. One stays
/// blocked when Discord keeps the pipe open without answering (the Windows
/// pipe has no read timeout), and connecting again would strand another one
/// every retry, so the worker waits until it has ended.
fn reader_busy(last: &Option<Arc<AtomicBool>>) -> bool {
    last.as_ref().is_some_and(|a| a.load(Ordering::Acquire))
}

/// What the worker does when it has no connection.
#[derive(Debug, PartialEq)]
enum Unconnected {
    /// Connect (and then send).
    Connect,
    /// A clear with nothing on Discord to clear: done.
    NothingToClear,
    /// Wait for an earlier connection's stuck reader to end first.
    WaitForReader,
}

/// `may_show`: an activity went out and no clear was answered since. While
/// the reader of a timed-out connection is alive, that pipe is still open
/// and Discord can keep showing "Hosting..." (or apply the late
/// SET_ACTIVITY), so a clear isn't done until it's sent on a new connection.
/// Once that reader ends the old pipe is closed, and Discord drops a closed
/// connection's activity on its own (which is also why quitting SyncCrate
/// never leaves one behind).
fn unconnected_step(clearing: bool, may_show: bool, reader_busy: bool) -> Unconnected {
    if clearing && !may_show {
        Unconnected::NothingToClear
    } else if reader_busy {
        Unconnected::WaitForReader
    } else {
        Unconnected::Connect
    }
}

/// Errors that get the slow retry: a Discord that stopped answering (each
/// attempt can leave a reader blocked) or refused what was sent (asking
/// again right away gets the same answer).
fn slow_retry(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::ConnectionRefused)
}

/// Connects and says hello. `last_reader` gets the new connection's reader
/// flag, also when the hello times out.
fn connect(last_reader: &mut Option<Arc<AtomicBool>>) -> std::io::Result<Conn> {
    let mut last = std::io::Error::other("Discord isn't running");
    for i in 0..10 {
        match open_socket(i) {
            Ok((reader, writer)) => {
                let mut c = Conn::start(reader, writer)?;
                *last_reader = Some(c.reader_alive.clone());
                c.request(&frame(0, &json!({ "v": 1, "client_id": CLIENT_ID })), REPLY_TIMEOUT)?; // READY
                return Ok(c);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

static TX: OnceLock<mpsc::Sender<Option<Presence>>> = OnceLock::new();

fn worker(rx: mpsc::Receiver<Option<Presence>>) {
    let pid = std::process::id();
    let mut pipe: Option<Conn> = None;
    // What should be shown, and what Discord last got.
    let mut wanted: Option<Presence> = None;
    let mut shown: Option<Option<Presence>> = None;
    let mut last_send = Instant::now().checked_sub(MIN_GAP).unwrap_or_else(Instant::now);
    // Set while Discord isn't reachable: don't try again before then.
    let mut retry_at: Option<Instant> = None;
    let mut nonce = 0u64;
    let mut last_reader: Option<Arc<AtomicBool>> = None;
    let mut may_show = false;
    loop {
        let wait = if shown.as_ref() == Some(&wanted) {
            RETRY
        } else if let Some(t) = retry_at {
            t.saturating_duration_since(Instant::now())
        } else {
            MIN_GAP.saturating_sub(last_send.elapsed())
        };
        match rx.recv_timeout(wait.max(Duration::from_millis(50))) {
            Ok(p) => {
                wanted = p;
                // Take everything queued: only the newest matters.
                while let Ok(p) = rx.try_recv() {
                    wanted = p;
                }
                if last_send.elapsed() < MIN_GAP || retry_at.is_some_and(|t| t > Instant::now()) {
                    continue;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if shown.as_ref() == Some(&wanted) {
            continue;
        }
        if pipe.is_none() {
            match unconnected_step(wanted.is_none(), may_show, reader_busy(&last_reader)) {
                Unconnected::Connect => {}
                Unconnected::NothingToClear => {
                    shown = Some(None);
                    continue;
                }
                Unconnected::WaitForReader => {
                    retry_at = Some(Instant::now() + RETRY);
                    continue;
                }
            }
            match connect(&mut last_reader) {
                Ok(p) => {
                    pipe = Some(p);
                    retry_at = None;
                }
                Err(e) if wanted.is_none() && e.kind() != std::io::ErrorKind::TimedOut => {
                    // No Discord to talk to, and the old pipe is closed (its
                    // reader ended), which cleared the activity already.
                    may_show = false;
                    shown = Some(None);
                    continue;
                }
                Err(_) => {
                    // Not running (or not logged in yet): try again later.
                    retry_at = Some(Instant::now() + RETRY);
                    continue;
                }
            }
        }
        let p = pipe.as_mut().expect("connected above");
        nonce += 1;
        // Set before sending: a request that times out may still be applied.
        may_show |= wanted.is_some();
        let sent = p.request(&frame(1, &activity_payload(wanted.as_ref(), pid, nonce)), REPLY_TIMEOUT);
        last_send = Instant::now();
        match sent {
            Ok(()) => {
                may_show = wanted.is_some();
                shown = Some(wanted.clone());
            }
            Err(e) => {
                // Discord quit or restarted: reconnect on the next round. A
                // Discord that stopped answering or refused gets the slower retry.
                pipe = None;
                shown = None;
                if slow_retry(&e) {
                    retry_at = Some(Instant::now() + RETRY);
                }
            }
        }
    }
}

fn sender() -> &'static mpsc::Sender<Option<Presence>> {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name("discord-presence".into()).spawn(move || worker(rx)).expect("spawn discord thread");
        tx
    })
}

/// Show (or with `None`, clear) SyncCrate on the user's Discord profile.
#[tauri::command]
pub fn set_discord_presence(presence: Option<Presence>) {
    if CLIENT_ID.is_empty() {
        return;
    }
    let _ = sender().send(presence);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_opcode_length_and_json() {
        let f = frame(1, &json!({ "a": 1 }));
        assert_eq!(&f[..4], &1u32.to_le_bytes());
        assert_eq!(&f[4..8], &7u32.to_le_bytes());
        assert_eq!(&f[8..], br#"{"a":1}"#);
    }

    #[test]
    fn payload_shows_two_lines_a_button_and_clears_with_none() {
        let p = Presence { details: "Hosting The Sims 4".into(), state: "2 friends connected".into(), start: Some(1_700_000_000) };
        let v = activity_payload(Some(&p), 42, 7);
        assert_eq!(v["cmd"], "SET_ACTIVITY");
        assert_eq!(v["args"]["pid"], 42);
        assert_eq!(v["args"]["activity"]["details"], "Hosting The Sims 4");
        assert_eq!(v["args"]["activity"]["timestamps"]["start"], 1_700_000_000u64);
        assert_eq!(v["args"]["activity"]["buttons"][0]["url"], WEBSITE);
        assert!(activity_payload(None, 42, 8)["args"]["activity"].is_null());
    }

    /// A pipe end that blocks on read until the test drops `_hold`, like a
    /// Discord that accepted the connection and then hung.
    struct Silent(mpsc::Receiver<()>);
    impl Read for Silent {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            let _ = self.0.recv();
            Ok(0)
        }
    }

    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_discord_that_never_answers_times_out_instead_of_hanging() {
        let (_hold, rx) = mpsc::channel();
        let sink = Sink::default();
        let mut c = Conn::start(Box::new(Silent(rx)), Box::new(sink.clone())).unwrap();
        let started = Instant::now();
        let err = c.request(&frame(0, &json!({ "v": 1 })), Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!sink.0.lock().unwrap().is_empty(), "the frame went out before the wait");
    }

    #[test]
    fn a_stuck_reader_blocks_reconnecting_until_it_ends() {
        let (hold, rx) = mpsc::channel::<()>();
        let mut c = Conn::start(Box::new(Silent(rx)), Box::new(Sink::default())).unwrap();
        let last = Some(c.reader_alive.clone());
        assert!(c.request(b"x", Duration::from_millis(50)).is_err());
        drop(c);
        // Dropping the connection doesn't end a reader stuck in a read.
        std::thread::sleep(Duration::from_millis(100));
        assert!(reader_busy(&last));
        // Discord answers or closes the pipe: the reader ends and a new attempt may start.
        drop(hold);
        let started = Instant::now();
        while reader_busy(&last) && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!reader_busy(&last));
        assert!(!reader_busy(&None));
    }

    #[test]
    fn replies_are_read_one_frame_per_request() {
        let mut replies = frame(1, &json!({ "evt": "READY" }));
        replies.extend(frame(1, &json!({ "cmd": "SET_ACTIVITY" })));
        let mut c = Conn::start(Box::new(std::io::Cursor::new(replies)), Box::new(Sink::default())).unwrap();
        let t = Duration::from_secs(5);
        c.request(b"a", t).unwrap();
        c.request(b"b", t).unwrap();
        // The pipe is now empty: a third request sees it closed, not a hang.
        assert!(c.request(b"c", t).is_err());
    }

    #[test]
    fn a_close_or_an_error_reply_is_a_refusal() {
        let t = Duration::from_secs(5);
        // Handshake answered with CLOSE (e.g. an unknown client id).
        let close = frame(OP_CLOSE, &json!({ "code": 4000, "message": "Invalid Client ID" }));
        let mut c = Conn::start(Box::new(std::io::Cursor::new(close)), Box::new(Sink::default())).unwrap();
        let e = c.request(b"hello", t).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused);
        assert!(e.to_string().contains("Invalid Client ID"), "{e}");
        assert!(slow_retry(&e), "not the 5 s fast path");

        // READY, then SET_ACTIVITY answered with an ERROR event.
        let mut replies = frame(1, &json!({ "cmd": "DISPATCH", "evt": "READY" }));
        replies.extend(frame(1, &json!({ "cmd": "SET_ACTIVITY", "evt": "ERROR", "data": { "code": 4000, "message": "activity is invalid" } })));
        let mut c = Conn::start(Box::new(std::io::Cursor::new(replies)), Box::new(Sink::default())).unwrap();
        c.request(b"hello", t).unwrap();
        let e = c.request(b"activity", t).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused);
        assert!(e.to_string().contains("activity is invalid"), "{e}");
        assert!(slow_retry(&e));
        assert!(!slow_retry(&std::io::Error::from(std::io::ErrorKind::BrokenPipe)), "Discord restarting reconnects soon");
    }

    #[test]
    fn a_clear_after_a_timeout_waits_and_is_sent() {
        // Never showed anything: a clear needs no connection.
        assert_eq!(unconnected_step(true, false, false), Unconnected::NothingToClear);
        // SET_ACTIVITY timed out and its reader still holds the pipe open:
        // Discord may show "Hosting..." until a clear gets through.
        assert_eq!(unconnected_step(true, true, true), Unconnected::WaitForReader);
        assert_eq!(unconnected_step(true, true, false), Unconnected::Connect);
        // Showing something works the same way.
        assert_eq!(unconnected_step(false, false, true), Unconnected::WaitForReader);
        assert_eq!(unconnected_step(false, true, false), Unconnected::Connect);
    }

    #[test]
    fn text_fits_discords_limits() {
        assert_eq!(fit("x").chars().count(), 3, "padded up to Discord's 2-character minimum");
        assert_eq!(fit(&"y".repeat(500)).chars().count(), 128);
        assert_eq!(fit("a\u{7}b"), "ab");
    }
}
