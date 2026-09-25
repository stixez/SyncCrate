//! Session chat. The host keeps the session's log; clients pull it.
//!
//! Why pull instead of the host pushing: a client reads host messages on the
//! same stream that file transfers and manifest requests use, and those
//! readers expect an exact reply (`request_file` holds the stream lock for a
//! whole file). An unsolicited `ChatBatch` arriving mid-file would desync
//! them. So the host only ever sends chat as the reply to a client's
//! `ChatSync`, and the client does that round trip in its idle loop while it
//! holds the stream lock. During a sync the client's chat simply waits.
//!
//! Compatibility: `ChatSync`/`ChatBatch` are new `Message` variants, which
//! an older peer can't parse, so each side only uses them when the other
//! advertised `FEATURE` in Hello/Welcome `features`.
//!
//! Everything a peer sends is untrusted: text and names are cleaned and
//! capped, a client's messages are rate-limited, and system lines ("Sam
//! finished syncing 42 files") are composed by the host from typed fields,
//! never taken as text from a peer.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const FEATURE: &str = "chat";
pub const MAX_TEXT_CHARS: usize = 500;
const MAX_NAME_CHARS: usize = 64;
/// Kept per session; older lines scroll away.
pub const MAX_LOG: usize = 500;
/// Per `ChatBatch`; a client that's further behind catches up over polls.
pub const MAX_BATCH: usize = 100;
/// Per `ChatSync` from a client.
pub const MAX_OUTGOING: usize = 10;
/// Queued on a client while it can't reach the host (e.g. mid-sync).
pub const MAX_OUTBOX: usize = 20;
/// Per client: at most this many messages per `RATE_WINDOW_SECS`.
const RATE_MAX: usize = 10;
const RATE_WINDOW_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    /// Host-assigned, increasing; clients ask for everything after the last one seen.
    pub seq: u64,
    pub from: String,
    pub text: String,
    pub at: u64,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChatLog {
    pub messages: VecDeque<ChatMessage>,
    /// Client side: typed but not yet delivered to the host.
    pub outbox: Vec<String>,
    /// Chat works in this session: always when hosting; as a client, only
    /// if the host listed `FEATURE` (hosts before 0.6.0 don't).
    pub available: bool,
    /// Client side: files the last sync received, reported on the next poll.
    #[serde(skip)]
    pub pending_synced: Option<u64>,
    #[serde(skip)]
    next_seq: u64,
}

/// Strip control characters (newlines included: one line per message),
/// trim, cap. `None` if nothing is left.
pub fn clean_text(text: &str, max: usize) -> Option<String> {
    let s: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s: String = s.trim().chars().take(max).collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn clean_name(name: &str) -> String {
    clean_text(name, MAX_NAME_CHARS).unwrap_or_else(|| "Someone".into())
}

impl ChatLog {
    pub fn last_seq(&self) -> u64 {
        self.messages.back().map_or(0, |m| m.seq)
    }

    /// Host side: append a line and return it.
    pub fn post(&mut self, from: &str, text: &str, system: bool, now: u64) -> Option<ChatMessage> {
        let text = clean_text(text, MAX_TEXT_CHARS)?;
        self.next_seq = self.next_seq.max(self.last_seq()) + 1;
        let msg = ChatMessage { seq: self.next_seq, from: clean_name(from), text, at: now, system };
        self.messages.push_back(msg.clone());
        while self.messages.len() > MAX_LOG {
            self.messages.pop_front();
        }
        Some(msg)
    }

    /// Host side: what a client that has seen up to `since` should get next.
    pub fn batch_after(&self, since: u64) -> Vec<ChatMessage> {
        self.messages.iter().filter(|m| m.seq > since).take(MAX_BATCH).cloned().collect()
    }

    /// Client side: merge a host batch. Validates and dedupes by seq, keeps
    /// order, and returns whether anything new arrived.
    pub fn merge_batch(&mut self, batch: Vec<ChatMessage>) -> bool {
        let mut changed = false;
        for m in batch.into_iter().take(MAX_BATCH) {
            if m.seq <= self.last_seq() {
                continue;
            }
            let Some(text) = clean_text(&m.text, MAX_TEXT_CHARS) else { continue };
            self.messages.push_back(ChatMessage { seq: m.seq, from: clean_name(&m.from), text, at: m.at, system: m.system });
            changed = true;
        }
        while self.messages.len() > MAX_LOG {
            self.messages.pop_front();
        }
        changed
    }

    /// Client side: queue a line for the next poll.
    pub fn queue(&mut self, text: &str) -> Result<(), String> {
        let text = clean_text(text, MAX_TEXT_CHARS).ok_or("Type a message first.")?;
        if self.outbox.len() >= MAX_OUTBOX {
            return Err("Slow down — messages are still being sent.".into());
        }
        self.outbox.push(text);
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = ChatLog::default();
    }
}

/// Host side, one per connected client.
#[derive(Debug, Default)]
pub struct RateLimiter {
    sent: VecDeque<u64>,
}

impl RateLimiter {
    pub fn allow(&mut self, now: u64) -> bool {
        while self.sent.front().is_some_and(|t| now.saturating_sub(*t) >= RATE_WINDOW_SECS) {
            self.sent.pop_front();
        }
        if self.sent.len() >= RATE_MAX {
            return false;
        }
        self.sent.push_back(now);
        true
    }
}

/// Host side of one `ChatSync`: post the client's lines (rate-limited) and a
/// "finished syncing" line if reported, then answer with everything after
/// `since`. Returns (reply, whether the log changed).
pub fn host_sync(
    log: &mut ChatLog,
    limiter: &mut RateLimiter,
    peer_name: &str,
    since: u64,
    outgoing: Vec<String>,
    synced_files: Option<u64>,
    now: u64,
) -> (Vec<ChatMessage>, bool) {
    let mut changed = false;
    for text in outgoing.into_iter().take(MAX_OUTGOING) {
        if !limiter.allow(now) {
            break;
        }
        changed |= log.post(peer_name, &text, false, now).is_some();
    }
    if let Some(n) = synced_files.filter(|n| *n > 0) {
        let files = if n == 1 { "file".to_string() } else { "files".to_string() };
        changed |= log.post(peer_name, &format!("{} finished syncing {n} {files}", clean_name(peer_name)), true, now).is_some();
    }
    (log.batch_after(since), changed)
}

pub fn supports(features: &[String]) -> bool {
    features.iter().any(|f| f == FEATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cleaned_and_capped() {
        assert_eq!(clean_text("  hi\nthere\u{0007} ", 500).as_deref(), Some("hi there"));
        assert_eq!(clean_text(" \n\t ", 500), None);
        assert_eq!(clean_text(&"x".repeat(900), MAX_TEXT_CHARS).unwrap().chars().count(), MAX_TEXT_CHARS);
        assert_eq!(clean_name(""), "Someone");
    }

    #[test]
    fn host_log_assigns_increasing_seq_and_scrolls() {
        let mut log = ChatLog::default();
        for i in 0..(MAX_LOG + 5) {
            log.post("Host", &format!("m{i}"), false, 1);
        }
        assert_eq!(log.messages.len(), MAX_LOG);
        assert_eq!(log.last_seq(), (MAX_LOG + 5) as u64);
        assert_eq!(log.messages.front().unwrap().seq, 6);
        assert!(log.post("Host", "   ", false, 1).is_none(), "empty lines are dropped");
        let b = log.batch_after(0);
        assert_eq!(b.len(), MAX_BATCH);
        assert_eq!(b[0].seq, 6);
        assert!(log.batch_after(log.last_seq()).is_empty());
    }

    #[test]
    fn client_merge_dedupes_orders_and_cleans() {
        let mut host = ChatLog::default();
        host.post("Host", "one", false, 1);
        host.post("Host", "two", false, 1);
        let mut client = ChatLog::default();
        assert!(client.merge_batch(host.batch_after(0)));
        assert!(!client.merge_batch(host.batch_after(0)), "a repeated batch adds nothing");
        let evil = ChatMessage { seq: 99, from: "\u{202e}x\n".repeat(50), text: "\u{0000}".into(), at: 0, system: false };
        assert!(!client.merge_batch(vec![evil]), "an empty-after-cleaning line is dropped");
        let long = ChatMessage { seq: 100, from: "A".repeat(200), text: "y".repeat(2000), at: 0, system: false };
        assert!(client.merge_batch(vec![long]));
        let last = client.messages.back().unwrap();
        assert_eq!(last.text.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(last.from.chars().count(), MAX_NAME_CHARS);
        assert_eq!(client.messages.len(), 3);
    }

    #[test]
    fn outbox_is_capped() {
        let mut c = ChatLog::default();
        assert!(c.queue("  ").is_err());
        for _ in 0..MAX_OUTBOX {
            c.queue("hi").unwrap();
        }
        assert!(c.queue("hi").is_err());
    }

    #[test]
    fn host_sync_rate_limits_and_composes_system_lines() {
        let mut log = ChatLog::default();
        let mut rl = RateLimiter::default();
        let flood: Vec<String> = (0..50).map(|i| format!("spam {i}")).collect();
        let (reply, changed) = host_sync(&mut log, &mut rl, "Sam", 0, flood, None, 100);
        assert!(changed);
        assert_eq!(reply.len(), MAX_OUTGOING, "only MAX_OUTGOING per sync");
        let since = log.last_seq();
        let (reply, _) = host_sync(&mut log, &mut rl, "Sam", since, vec!["more".into()], None, 101);
        assert!(reply.is_empty(), "rate limit reached within the window");
        let since = log.last_seq();
        let (reply, _) = host_sync(&mut log, &mut rl, "Sam", since, vec!["later".into()], Some(42), 100 + RATE_WINDOW_SECS);
        assert_eq!(reply.len(), 2);
        assert_eq!(reply[0].text, "later");
        assert!(reply[1].system);
        assert_eq!(reply[1].text, "Sam finished syncing 42 files");
        let since = log.last_seq();
        let (reply, _) = host_sync(&mut log, &mut rl, "Sam", since, vec![], Some(0), 200);
        assert!(reply.is_empty(), "a zero-file sync isn't announced");
    }
}
