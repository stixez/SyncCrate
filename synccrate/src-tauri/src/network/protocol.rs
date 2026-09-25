use crate::chat::ChatMessage;
use crate::crews::{CrewHello, CrewWelcome};
use crate::state::{FileInfo, FileManifest, GameInfo};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::network::stream::PeerStream;
use tokio::net::TcpStream;

/// Maximum message size: 10 MB (sufficient for large manifests)
const MAX_MESSAGE_SIZE: usize = 10_000_000;

/// Maximum number of files allowed in a received manifest
pub const MAX_MANIFEST_FILES: usize = 50_000;

/// Timeout for network read operations
const RECV_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for network write operations
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Hello {
        name: String,
        version: String,
        pin: Option<String>,
        #[serde(default)]
        supports_compression: bool,
        /// The client's selected game. `None` from clients older than 0.5.6.
        #[serde(default)]
        game_id: Option<String>,
        /// Crew fields (0.6.0+). Omitted when empty, so a non-crew Hello is
        /// byte-for-byte what 0.5.6 sent; older hosts ignore unknown fields.
        /// Over TCP `node_id` is only a claim (see `crews` module docs).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        crews: Vec<CrewHello>,
        /// Optional protocol features this side speaks (e.g. `chat::FEATURE`).
        /// New `Message` variants are only sent to a peer that listed them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        features: Vec<String>,
    },
    Welcome {
        name: String,
        version: String,
        #[serde(default)]
        supports_compression: bool,
        /// The game this host is sharing. `None` from hosts older than 0.5.6.
        #[serde(default)]
        game_id: Option<String>,
        /// Crew fields (0.6.0+), same compatibility rules as in Hello. `crews`
        /// only answers crews the client named in its Hello.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        crews: Vec<CrewWelcome>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        features: Vec<String>,
    },
    ManifestRequest,
    ManifestResponse { manifest: FileManifest },
    FileRequest { path: String },
    FileHeader { path: String, size: u64, hash: String },
    FileChunk {
        data: String,
        offset: u64,
        #[serde(default)]
        compressed: bool,
    },
    FileComplete { path: String },
    SyncComplete,
    Error { message: String },
    Disconnect,
    GameInfoExchange { game_info: GameInfo },
    Ping,
    /// Client → host chat poll (only to a host whose Welcome listed
    /// `chat::FEATURE`): our new lines, and the last seq we've seen.
    ChatSync {
        since: u64,
        #[serde(default)]
        outgoing: Vec<String>,
        /// Files our last sync received; the host announces it.
        #[serde(default)]
        synced_files: Option<u64>,
    },
    /// Client → host (only if the host lists `offers::FEATURE`): our offer's
    /// file list (the first time), or just a status poll.
    OfferSync {
        #[serde(default)]
        files: Option<Vec<FileInfo>>,
    },
    /// Host → client, only as the reply to `OfferSync`.
    OfferStatus { accepted: Vec<String>, declined: Vec<String> },
    /// Host → client, only as the reply to an offered file's upload
    /// (`FileHeader` + chunks + `FileComplete` from the client).
    OfferResult {
        path: String,
        ok: bool,
        #[serde(default)]
        message: String,
    },
    /// Host → client, only ever as the reply to `ChatSync` (see `chat` docs).
    ChatBatch {
        messages: Vec<ChatMessage>,
        /// How many of the `ChatSync`'s lines the host posted (the rest hit
        /// its rate limit; the client keeps them queued).
        #[serde(default)]
        accepted: Option<usize>,
    },
}

/// Error a host sends when a client joins with a different game selected.
/// Machine-readable so the client can offer "switch to <game> and join". A
/// client with the wrong game selected would otherwise plan to download the
/// host's files into its own game's folder (real bug: Sims 4 mods synced into
/// Euro Truck Simulator 2).
pub const WRONG_GAME_PREFIX: &str = "wrong-game:";

pub fn wrong_game_error(host_game: &str) -> String {
    format!("{WRONG_GAME_PREFIX}{host_game}")
}

/// The host's game id if `message` is a wrong-game error.
pub fn parse_wrong_game(message: &str) -> Option<&str> {
    message.strip_prefix(WRONG_GAME_PREFIX).filter(|g| !g.is_empty())
}

/// Whether the two sides are sharing different games. Unknown (an older
/// peer that doesn't send its game) is allowed so mixed versions still connect.
pub fn games_conflict(ours: &str, theirs: Option<&str>) -> bool {
    matches!(theirs, Some(t) if !t.is_empty() && !ours.is_empty() && t != ours)
}

pub async fn send_message(stream: &mut PeerStream, msg: &Message) -> Result<(), String> {
    let json = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let len: u32 = json.len().try_into().map_err(|_| "Message too large to send")?;
    tokio::time::timeout(SEND_TIMEOUT, async {
        stream.write_all(&len.to_be_bytes()).await.map_err(|e| e.to_string())?;
        stream.write_all(&json).await.map_err(|e| e.to_string())?;
        stream.flush().await.map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Connection timed out writing message".to_string())?
}

/// A Hello is a few hundred bytes (a name, a PIN, up to 32 crew ids); an
/// unauthenticated peer gets no more than this, and not for long.
pub const MAX_HELLO_SIZE: usize = 64 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Bytes read per step: the buffer grows as data really arrives, so a length
/// prefix alone can't make us allocate 10 MB.
const READ_STEP: usize = 64 * 1024;

/// Internal: reads one length-prefixed JSON message without a timeout wrapper.
async fn recv_message_raw(stream: &mut PeerStream, max: usize) -> Result<Message, String> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > max {
        return Err(format!("Message too large: {} bytes (max {})", len, max));
    }

    let mut buf = Vec::with_capacity(len.min(READ_STEP));
    let mut step = vec![0u8; len.clamp(1, READ_STEP)];
    while buf.len() < len {
        let n = (len - buf.len()).min(READ_STEP);
        stream.read_exact(&mut step[..n]).await.map_err(|e| e.to_string())?;
        buf.extend_from_slice(&step[..n]);
    }

    let msg: Message = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;

    // Validate manifest size from untrusted peers
    if let Message::ManifestResponse { ref manifest } = msg {
        if manifest.files.len() > MAX_MANIFEST_FILES {
            return Err(format!(
                "Manifest too large: {} files (max {})",
                manifest.files.len(),
                MAX_MANIFEST_FILES
            ));
        }
    }

    Ok(msg)
}

pub async fn recv_message(stream: &mut PeerStream) -> Result<Message, String> {
    tokio::time::timeout(RECV_TIMEOUT, recv_message_raw(stream, MAX_MESSAGE_SIZE))
        .await
        .map_err(|_| "Connection timed out reading message".to_string())?
}

/// The host's first read from a new, unauthenticated peer: small and quick.
pub async fn recv_hello(stream: &mut PeerStream) -> Result<Message, String> {
    tokio::time::timeout(HELLO_TIMEOUT, recv_message_raw(stream, MAX_HELLO_SIZE))
        .await
        .map_err(|_| "Timed out waiting for Hello".to_string())?
}

/// Wait up to `timeout` for a message to *start* arriving, then read it fully.
/// Returns Ok(Some(msg)) on success, Ok(None) if nothing arrived, Err on connection error.
///
/// Cancel-safe: the wait buffers whatever a single read returns (see `PeerStream`). Previously the
/// whole read was wrapped in the timeout, so a timeout that fired after the
/// length prefix was read (but before the body) silently dropped bytes and
/// desynchronised the stream — surfacing later as random "connection lost" /
/// JSON errors, most often on slow Wi-Fi.
pub async fn try_recv_message(stream: &mut PeerStream, timeout: Duration) -> Result<Option<Message>, String> {
    match stream.wait_readable(timeout).await {
        Ok(None) => Ok(None),
        Ok(Some(false)) => Err("Connection closed by peer".to_string()),
        Ok(Some(true)) => recv_message(stream).await.map(Some),
        Err(e) => Err(e.to_string()),
    }
}

/// Configure TCP keepalive on a stream to prevent NAT/firewall idle timeouts.
/// Sends a keepalive probe every 15 seconds after 15 seconds of idle.
pub fn configure_keepalive(stream: &TcpStream) {
    let sock_ref = socket2::SockRef::from(stream);
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(15))
        .with_interval(Duration::from_secs(15));
    if let Err(e) = sock_ref.set_tcp_keepalive(&keepalive) {
        log::warn!("Failed to set TCP keepalive: {}", e);
    }
}

/// Leave room under `MAX_MESSAGE_SIZE` for the rest of the Welcome.
pub const MAX_CREW_WELCOME_BYTES: usize = MAX_MESSAGE_SIZE - 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_conflict_only_when_both_known_and_different() {
        assert!(games_conflict("ets2", Some("sims4")));
        assert!(!games_conflict("sims4", Some("sims4")));
        // Older peers don't send a game: allow, so mixed versions still connect.
        assert!(!games_conflict("sims4", None));
        assert!(!games_conflict("sims4", Some("")));
        assert!(!games_conflict("", Some("sims4")));
    }

    #[test]
    fn wrong_game_error_round_trips() {
        let e = wrong_game_error("sims4");
        assert_eq!(parse_wrong_game(&e), Some("sims4"));
        assert_eq!(parse_wrong_game("Invalid PIN"), None);
        assert_eq!(parse_wrong_game("wrong-game:"), None);
    }

    #[test]
    fn hello_without_game_id_still_parses() {
        // A 0.5.5 client's Hello has no game_id field.
        let json = r#"{"Hello":{"name":"A","version":"0.5.5","pin":null,"supports_compression":true}}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Hello { game_id, .. } => assert!(game_id.is_none()),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn crew_fields_are_optional_both_ways() {
        // A 0.5.6 Welcome (no crew fields) parses on a new client.
        let json = r#"{"Welcome":{"name":"H","version":"0.5.6","supports_compression":true,"game_id":"sims4"}}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Welcome { node_id, crews, .. } => assert!(node_id.is_none() && crews.is_empty()),
            _ => panic!("expected Welcome"),
        }
        // A new Hello without crews serialises exactly like a 0.5.6 one.
        let hello = Message::Hello {
            name: "A".into(),
            version: "0.6.0".into(),
            pin: None,
            supports_compression: true,
            game_id: Some("sims4".into()),
            node_id: None,
            crews: vec![],
            features: vec![],
        };
        let s = serde_json::to_string(&hello).unwrap();
        assert!(!s.contains("node_id") && !s.contains("crews") && !s.contains("features"), "{s}");
        // Unknown extra fields (what an old peer sees from a new one) are ignored.
        let json = r#"{"Hello":{"name":"A","version":"0.7.0","pin":null,"game_id":"sims4","node_id":"ab","crews":[{"id":"x"}],"future":1}}"#;
        assert!(serde_json::from_str::<Message>(json).is_ok());
    }
}
