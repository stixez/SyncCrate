use crate::state::{FileManifest, GameInfo};
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
    },
    Welcome {
        name: String,
        version: String,
        #[serde(default)]
        supports_compression: bool,
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

/// Internal: reads one length-prefixed JSON message without a timeout wrapper.
async fn recv_message_raw(stream: &mut PeerStream) -> Result<Message, String> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_MESSAGE_SIZE {
        return Err(format!("Message too large: {} bytes (max {})", len, MAX_MESSAGE_SIZE));
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.map_err(|e| e.to_string())?;

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
    tokio::time::timeout(RECV_TIMEOUT, recv_message_raw(stream))
        .await
        .map_err(|_| "Connection timed out reading message".to_string())?
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
