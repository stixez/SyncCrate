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

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

fn read_frame(p: &mut dyn Pipe) -> std::io::Result<()> {
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

#[cfg(windows)]
fn open_socket(i: u8) -> std::io::Result<Box<dyn Pipe>> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}"))?;
    Ok(Box::new(f))
}

#[cfg(unix)]
fn open_socket(i: u8) -> std::io::Result<Box<dyn Pipe>> {
    let mut last = std::io::Error::other("no Discord socket");
    let bases: Vec<String> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|v| std::env::var(v).ok()).chain(["/tmp".to_string()]).collect();
    for base in bases {
        // Plain, Flatpak and Snap Discord.
        for sub in ["", "app/com.discordapp.Discord/", "snap.discord/"] {
            match std::os::unix::net::UnixStream::connect(format!("{base}/{sub}discord-ipc-{i}")) {
                Ok(s) => {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                    return Ok(Box::new(s));
                }
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

fn connect() -> std::io::Result<Box<dyn Pipe>> {
    let mut last = std::io::Error::other("Discord isn't running");
    for i in 0..10 {
        match open_socket(i) {
            Ok(mut p) => {
                p.write_all(&frame(0, &json!({ "v": 1, "client_id": CLIENT_ID })))?;
                read_frame(&mut *p)?; // READY
                return Ok(p);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

static TX: OnceLock<mpsc::Sender<Option<Presence>>> = OnceLock::new();

fn worker(rx: mpsc::Receiver<Option<Presence>>) {
    let pid = std::process::id();
    let mut pipe: Option<Box<dyn Pipe>> = None;
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
        let sent = p.write_all(&frame(1, &activity_payload(wanted.as_ref(), pid, nonce))).and_then(|_| read_frame(&mut **p));
        last_send = Instant::now();
        match sent {
            Ok(()) => shown = Some(wanted.clone()),
            Err(_) => {
                // Discord quit or restarted: reconnect on the next round.
                pipe = None;
                shown = None;
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

    #[test]
    fn text_fits_discords_limits() {
        assert_eq!(fit("x").chars().count(), 3, "padded up to Discord's 2-character minimum");
        assert_eq!(fit(&"y".repeat(500)).chars().count(), 128);
        assert_eq!(fit("a\u{7}b"), "ab");
    }
}
