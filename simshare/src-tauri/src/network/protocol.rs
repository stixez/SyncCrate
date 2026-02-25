use crate::state::FileManifest;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Maximum message size: 10 MB (sufficient for large manifests)
const MAX_MESSAGE_SIZE: usize = 10_000_000;

/// Maximum number of files allowed in a received manifest
pub const MAX_MANIFEST_FILES: usize = 50_000;

/// Timeout for network read operations
const RECV_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Hello { name: String, version: String, pin: Option<String> },
    Welcome { name: String, version: String },
    ManifestRequest,
    ManifestResponse { manifest: FileManifest },
    FileRequest { path: String },
    FileHeader { path: String, size: u64, hash: String },
    FileChunk { data: String, offset: u64 },
    FileComplete { path: String },
    SyncComplete,
    Error { message: String },
    Disconnect,
}

pub async fn send_message(stream: &mut TcpStream, msg: &Message) -> Result<(), String> {
    let json = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let len: u32 = json.len().try_into().map_err(|_| "Message too large to send")?;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&json)
        .await
        .map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn recv_message(stream: &mut TcpStream) -> Result<Message, String> {
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(RECV_TIMEOUT, stream.read_exact(&mut len_buf))
        .await
        .map_err(|_| "Connection timed out reading message length".to_string())?
        .map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_MESSAGE_SIZE {
        return Err(format!("Message too large: {} bytes (max {})", len, MAX_MESSAGE_SIZE));
    }

    let mut buf = vec![0u8; len];
    tokio::time::timeout(RECV_TIMEOUT, stream.read_exact(&mut buf))
        .await
        .map_err(|_| "Connection timed out reading message body".to_string())?
        .map_err(|e| e.to_string())?;

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
