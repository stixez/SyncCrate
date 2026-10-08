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
use std::sync::mpsc;
use std::sync::OnceLock;
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

fn read_frame(p: &mut dyn Read) -> std::io::Result<()> {
    let mut hdr = [0u8; 8];
    p.read_exact(&mut hdr)?;
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    // Replies are small; a huge length means a wrong peer, not Discord.
    if len > 64 * 1024 {
        return Err(std::io::Error::other("oversized frame"));
    }
    let mut body = vec![0u8; len];
    p.read_exact(&mut body)
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
}

impl Conn {
    fn start(mut reader: Reader, writer: Writer) -> std::io::Result<Conn> {
        let (want, want_rx) = mpsc::channel::<()>();
        let (frames_tx, frames) = mpsc::channel();
        // Reads only on request: a synchronous Windows pipe serialises I/O on
        // the handle, so an idle read left pending would block the next write.
        std::thread::Builder::new().name("discord-ipc-read".into()).spawn(move || {
            while want_rx.recv().is_ok() {
                let r = read_frame(&mut *reader);
                let failed = r.is_err();
                if frames_tx.send(r).is_err() || failed {
                    return;
                }
            }
        })?;
        Ok(Conn { writer, want, frames })
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

fn connect() -> std::io::Result<Conn> {
    let mut last = std::io::Error::other("Discord isn't running");
    for i in 0..10 {
        match open_socket(i) {
            Ok((reader, writer)) => {
                let mut c = Conn::start(reader, writer)?;
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
            if wanted.is_none() {
                // Nothing to clear on a Discord we aren't connected to.
                shown = Some(None);
                continue;
            }
            match connect() {
                Ok(p) => {
                    pipe = Some(p);
                    retry_at = None;
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
        let sent = p.request(&frame(1, &activity_payload(wanted.as_ref(), pid, nonce)), REPLY_TIMEOUT);
        last_send = Instant::now();
        match sent {
            Ok(()) => shown = Some(wanted.clone()),
            Err(e) => {
                // Discord quit or restarted: reconnect on the next round. A
                // Discord that stopped answering gets the slower retry, since
                // each attempt can leave a reader blocked until it recovers.
                pipe = None;
                shown = None;
                if e.kind() == std::io::ErrorKind::TimedOut {
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
    fn text_fits_discords_limits() {
        assert_eq!(fit("x").chars().count(), 3, "padded up to Discord's 2-character minimum");
        assert_eq!(fit(&"y".repeat(500)).chars().count(), 128);
        assert_eq!(fit("a\u{7}b"), "ab");
    }
}
