use crate::network::protocol::{self, Message};
use crate::state::{AppState, GameInfo};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::Emitter;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Maximum file size we'll accept from a peer (2 GB)
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Maximum simultaneous peer connections a host will accept
const MAX_PEERS: usize = 8;

/// Maximum allowed length for a peer display name
const MAX_PEER_NAME_LEN: usize = 64;

/// Maximum decoded size of a single file chunk (1 MB)
const MAX_CHUNK_SIZE: usize = 1_048_576;

/// Maximum number of packs a peer can advertise
const MAX_PEER_PACKS: usize = 200;

/// Maximum string length for a pack code or name from a peer
const MAX_PACK_STRING_LEN: usize = 128;

/// Extensions that are already compressed — skip zstd for these.
const SKIP_COMPRESSION_EXTS: &[&str] = &[
    "zip", "7z", "rar", "gz", "bz2", "xz", "zst",
    "png", "jpg", "jpeg", "gif", "webp",
    "dds", "ogg", "mp3", "mp4", "flac", "avi", "mkv",
];

/// Minimum file size to bother compressing (1 KB).
const MIN_COMPRESS_SIZE: u64 = 1024;

/// Check if a file path has an already-compressed extension.
fn should_skip_compression(path: &str) -> bool {
    let lower = path.to_lowercase();
    SKIP_COMPRESSION_EXTS.iter().any(|ext| lower.ends_with(&format!(".{}", ext)))
}

/// Compress a chunk of data using zstd at level 3.
fn compress_chunk(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::encode_all(std::io::Cursor::new(data), 3)
        .map_err(|e| format!("Compression failed: {}", e))
}

/// Decompress a zstd-compressed chunk.
fn decompress_chunk(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::decode_all(std::io::Cursor::new(data))
        .map_err(|e| format!("Decompression failed: {}", e))
}

/// Sanitize GameInfo received from an untrusted peer.
fn sanitize_game_info(info: GameInfo) -> GameInfo {
    let game_version = info.game_version.map(|v| {
        v.chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect::<String>()
    }).filter(|v| !v.is_empty());

    let installed_packs: Vec<_> = info
        .installed_packs
        .into_iter()
        .take(MAX_PEER_PACKS)
        .map(|mut p| {
            p.id.code.truncate(MAX_PACK_STRING_LEN);
            p.name.truncate(MAX_PACK_STRING_LEN);
            p
        })
        .collect();

    GameInfo {
        game_version,
        installed_packs,
    }
}

static LISTENER_TOKEN: Mutex<Option<CancellationToken>> = Mutex::const_new(None);

async fn get_or_create_token() -> CancellationToken {
    let mut guard = LISTENER_TOKEN.lock().await;
    if let Some(ref token) = *guard {
        if !token.is_cancelled() {
            return token.clone();
        }
        // Clear stale cancelled token before creating a new one
        guard.take();
    }
    let token = CancellationToken::new();
    *guard = Some(token.clone());
    token
}

pub async fn reset_cancellation_token() {
    let mut guard = LISTENER_TOKEN.lock().await;
    if let Some(token) = guard.take() {
        token.cancel();
    }
}

/// Bind the TCP listener and return it. Call `run_listener` to start accepting.
/// Separated so the caller can detect port conflicts before spawning.
pub async fn bind_listener(port: u16) -> Result<TcpListener, String> {
    let addr = format!("0.0.0.0:{}", port);
    TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind port {}: {}", port, e))
}

/// Accept loop for an already-bound listener.
pub async fn run_listener(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    app: tauri::AppHandle,
) {
    let token = get_or_create_token().await;

    log::info!("Listening on {:?}", listener.local_addr());

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                log::info!("TCP listener shutting down");
                break;
            }
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                        continue;
                    }
                };

                let state = state.clone();
                let app = app.clone();

                // Enforce connection limit
                {
                    let app_state = state.lock().await;
                    if app_state.connections.len() >= MAX_PEERS {
                        log::warn!("Rejecting connection from {} — max peers ({}) reached", peer_addr, MAX_PEERS);
                        // Drop the stream immediately; peer will see a connection reset
                        drop(stream);
                        continue;
                    }
                }

                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, state, app, peer_addr.to_string()).await {
                        log::error!("Client handler error: {}", e);
                    }
                });
            }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    state: Arc<Mutex<AppState>>,
    app: tauri::AppHandle,
    _peer_addr: String,
) -> Result<(), String> {
    protocol::configure_keepalive(&stream);
    let stream = Arc::new(Mutex::new(stream));

    // Wait for Hello
    let msg = {
        let mut s = stream.lock().await;
        protocol::recv_message(&mut *s).await?
    };

    let mut use_compression = false;

    let (peer_name, peer_version, peer_pin) = match msg {
        Message::Hello { name, version, pin, supports_compression } => {
            // Sanitize: truncate and strip control characters
            let sanitized = name.chars()
                .filter(|c| !c.is_control())
                .take(MAX_PEER_NAME_LEN)
                .collect::<String>();
            let peer_supports_compression = supports_compression;
            use_compression = peer_supports_compression;
            (sanitized, version, pin)
        }
        _ => return Err("Expected Hello message".to_string()),
    };

    if peer_name.is_empty() {
        return Err("Peer sent empty name".to_string());
    }

    // Validate PIN if the host has one set
    {
        let app_state = state.lock().await;
        if let Some(ref expected_pin) = app_state.session_pin {
            match &peer_pin {
                Some(provided) if provided == expected_pin => {}
                _ => {
                    let mut s = stream.lock().await;
                    protocol::send_message(
                        &mut *s,
                        &Message::Error {
                            message: "Invalid PIN".to_string(),
                        },
                    )
                    .await?;
                    return Err("Peer provided invalid PIN".to_string());
                }
            }
        }
    }

    // Send Welcome
    {
        let app_state = state.lock().await;
        let our_name = app_state.session_name.clone();
        let mut s = stream.lock().await;
        protocol::send_message(
            &mut *s,
            &Message::Welcome {
                name: our_name,
                version: env!("CARGO_PKG_VERSION").to_string(),
                supports_compression: true,
            },
        )
        .await?;
    }

    // Add peer to state
    let peer_id = uuid::Uuid::new_v4().to_string();
    {
        let mut app_state = state.lock().await;
        let peer = crate::state::PeerInfo {
            id: peer_id.clone(),
            name: peer_name.clone(),
            ip: _peer_addr.split(':').next().unwrap_or("unknown").to_string(),
            port: 0,
            mod_count: 0,
            version: peer_version.clone(),
            pin_required: false,
            game_info: None,
        };
        app_state.connections.insert(
            peer_id.clone(),
            crate::state::PeerConnection {
                info: peer,
                stream: stream.clone(),
                remote_manifest: None,
                sync_plan: None,
                is_syncing: false,
                supports_compression: use_compression,
            },
        );
    }

    let _ = app.emit(
        "peer-connected",
        serde_json::json!({"name": &peer_name, "peer_id": &peer_id}),
    );

    // Send our game info to the peer
    {
        let app_state = state.lock().await;
        let active = &app_state.active_game;
        if let Some(info) = app_state.game_info.get(active) {
            let mut s = stream.lock().await;
            if let Err(e) = protocol::send_message(
                &mut *s,
                &Message::GameInfoExchange {
                    game_info: info.clone(),
                },
            )
            .await
            {
                log::warn!("Failed to send game info to peer '{}': {}", peer_name, e);
            }
        }
    }

    // Handle messages in a loop (5-minute idle timeout; TCP keepalive detects dead connections)
    let mut clean_disconnect = false;
    let mut disconnect_reason = String::new();
    let mut peer_files_sent: u64 = 0;
    loop {
        let msg = {
            let mut s = stream.lock().await;
            match protocol::try_recv_message(&mut *s, std::time::Duration::from_secs(300)).await {
                Ok(Some(m)) => m,
                Ok(None) => {
                    log::warn!("Idle timeout for peer '{}' ({})", peer_name, peer_id);
                    disconnect_reason = "Idle timeout (5 min)".to_string();
                    break;
                }
                Err(e) => {
                    log::warn!("Connection lost for peer '{}' ({}): {}", peer_name, peer_id, e);
                    disconnect_reason = e;
                    break;
                }
            }
        };

        match msg {
            Message::ManifestRequest => {
                let manifest = {
                    let app_state = state.lock().await;
                    let perms = &app_state.folder_permissions;
                    let mut filtered = app_state.local_manifest.clone();
                    filtered.files.retain(|_, info| crate::state::is_file_allowed(perms, &info.file_type));
                    filtered
                };
                let mut s = stream.lock().await;
                protocol::send_message(&mut *s, &Message::ManifestResponse { manifest }).await?;
            }
            Message::ManifestResponse { manifest } => {
                let mod_count = manifest.files.len();
                let mut app_state = state.lock().await;
                if let Some(conn) = app_state.connections.get_mut(&peer_id) {
                    conn.info.mod_count = mod_count;
                    conn.remote_manifest = Some(manifest);
                }
                drop(app_state);
                // Notify frontend so peer info updates
                let _ = app.emit(
                    "peer-game-info",
                    serde_json::json!({"peer_id": &peer_id}),
                );
            }
            Message::FileRequest { path } => {
                let base = {
                    let app_state = state.lock().await;

                    // Validate that the requested file is in an allowed folder
                    let allowed = app_state.local_manifest.files.get(&path)
                        .map(|info| crate::state::is_file_allowed(&app_state.folder_permissions, &info.file_type))
                        .unwrap_or(false);
                    if !allowed {
                        let mut s = stream.lock().await;
                        protocol::send_message(
                            &mut *s,
                            &Message::Error {
                                message: "File not available".to_string(),
                            },
                        )
                        .await?;
                        continue;
                    }

                    match app_state.active_game_path() {
                        Ok(p) => p,
                        Err(e) => {
                            let mut s = stream.lock().await;
                            protocol::send_message(
                                &mut *s,
                                &Message::Error {
                                    message: e,
                                },
                            )
                            .await?;
                            continue;
                        }
                    }
                };

                // Validate path stays within base directory
                let full_path = match crate::utils::safe_join(&base, &path) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("Path traversal blocked in FileRequest: {}", e);
                        let mut s = stream.lock().await;
                        protocol::send_message(
                            &mut *s,
                            &Message::Error {
                                message: "Invalid file path".to_string(),
                            },
                        )
                        .await?;
                        continue;
                    }
                };

                match tokio::fs::File::open(&full_path).await {
                    Ok(mut file) => {
                        // Get file size for the header
                        let metadata = file.metadata().await.map_err(|e| e.to_string())?;
                        let file_size = metadata.len();

                        // Determine if compression should be used for this file
                        let compress_this_file = use_compression
                            && file_size >= MIN_COMPRESS_SIZE
                            && !should_skip_compression(&path);

                        // Stream file: read in 64KB chunks, hash incrementally, send immediately
                        let chunk_size = 65536;
                        let mut buf = vec![0u8; chunk_size];
                        let mut hasher = Sha256::new();
                        let mut offset = 0u64;

                        // First pass: compute hash by streaming through the file
                        loop {
                            let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
                            if n == 0 { break; }
                            hasher.update(&buf[..n]);
                        }
                        let hash = hex::encode(hasher.finalize());

                        // Send header with known size and hash
                        let mut s = stream.lock().await;
                        protocol::send_message(
                            &mut *s,
                            &Message::FileHeader {
                                path: path.clone(),
                                size: file_size,
                                hash,
                            },
                        )
                        .await?;

                        // Emit initial progress for this file
                        let _ = app.emit(
                            "peer-download-progress",
                            serde_json::json!({
                                "peer_id": &peer_id,
                                "peer_name": &peer_name,
                                "file": &path,
                                "file_bytes_sent": 0u64,
                                "file_bytes_total": file_size,
                                "files_sent": peer_files_sent,
                            }),
                        );

                        // Second pass: re-read and send chunks
                        // Seek back to start
                        use tokio::io::AsyncSeekExt;
                        file.seek(std::io::SeekFrom::Start(0)).await.map_err(|e| e.to_string())?;

                        loop {
                            let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
                            if n == 0 { break; }

                            let (send_data, is_compressed) = if compress_this_file {
                                match compress_chunk(&buf[..n]) {
                                    Ok(compressed) if compressed.len() < n => (BASE64.encode(&compressed), true),
                                    _ => (BASE64.encode(&buf[..n]), false),
                                }
                            } else {
                                (BASE64.encode(&buf[..n]), false)
                            };

                            protocol::send_message(
                                &mut *s,
                                &Message::FileChunk {
                                    data: send_data,
                                    offset,
                                    compressed: is_compressed,
                                },
                            )
                            .await?;
                            offset += n as u64;

                            // Emit chunk progress to frontend
                            let _ = app.emit(
                                "peer-download-progress",
                                serde_json::json!({
                                    "peer_id": &peer_id,
                                    "peer_name": &peer_name,
                                    "file": &path,
                                    "file_bytes_sent": offset,
                                    "file_bytes_total": file_size,
                                    "files_sent": peer_files_sent,
                                }),
                            );
                        }

                        peer_files_sent += 1;
                        protocol::send_message(&mut *s, &Message::FileComplete { path }).await?;

                        // Emit completion for this file
                        let null_file: Option<&str> = None;
                        let _ = app.emit(
                            "peer-download-progress",
                            serde_json::json!({
                                "peer_id": &peer_id,
                                "peer_name": &peer_name,
                                "file": null_file,
                                "file_bytes_sent": 0u64,
                                "file_bytes_total": 0u64,
                                "files_sent": peer_files_sent,
                            }),
                        );
                    }
                    Err(e) => {
                        log::error!("File read error: {}", e);
                        let mut s = stream.lock().await;
                        protocol::send_message(
                            &mut *s,
                            &Message::Error {
                                message: "File not available".to_string(),
                            },
                        )
                        .await?;
                    }
                }
            }
            Message::GameInfoExchange { game_info } => {
                let sanitized = sanitize_game_info(game_info);
                let mut app_state = state.lock().await;
                if let Some(conn) = app_state.connections.get_mut(&peer_id) {
                    conn.info.game_info = Some(sanitized.clone());
                }
                let _ = app.emit(
                    "peer-game-info",
                    serde_json::json!({"peer_id": &peer_id, "game_info": &sanitized}),
                );
            }
            Message::Ping => {
                // Keepalive — no-op, resets the idle timeout
            }
            Message::Disconnect => {
                clean_disconnect = true;
                break;
            }
            other => {
                log::warn!("Unexpected message from peer: {:?}", other);
            }
        }
    }

    // Always emit disconnect event (whether clean or unexpected)
    let _ = app.emit(
        "peer-disconnected",
        serde_json::json!({
            "name": &peer_name,
            "peer_id": &peer_id,
            "clean": clean_disconnect,
            "reason": if clean_disconnect { "User disconnected".to_string() } else if disconnect_reason.is_empty() { "Unknown".to_string() } else { disconnect_reason },
        }),
    );

    // Clean up connection from state
    {
        let mut app_state = state.lock().await;
        app_state.connections.remove(&peer_id);
    }

    Ok(())
}

pub async fn connect_to_host(
    ip: &str,
    port: u16,
    peer_id: &str,
    state: Arc<Mutex<AppState>>,
    app: tauri::AppHandle,
    pin: Option<String>,
) -> Result<(), String> {
    let addr = format!("{}:{}", ip, port);
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| "Connection timed out (10s)".to_string())?
    .map_err(|e| format!("Connection failed: {}", e))?;

    protocol::configure_keepalive(&stream);
    let stream = Arc::new(Mutex::new(stream));

    // Send Hello with our display name (not the session/peer name)
    {
        let app_state = state.lock().await;
        let our_name = app_state.local_display_name.clone();
        let mut s = stream.lock().await;
        protocol::send_message(
            &mut *s,
            &Message::Hello {
                name: our_name,
                version: env!("CARGO_PKG_VERSION").to_string(),
                pin: pin.clone(),
                supports_compression: true,
            },
        )
        .await?;
    }

    // Wait for Welcome (or Error if PIN was rejected)
    let (host_name, host_version, host_supports_compression) = {
        let mut s = stream.lock().await;
        let msg = protocol::recv_message(&mut *s).await?;
        match msg {
            Message::Welcome { name, version, supports_compression } => (name, version, supports_compression),
            Message::Error { message } => return Err(message),
            _ => return Err("Expected Welcome message".to_string()),
        }
    };

    // Send our game info to the host
    {
        let app_state = state.lock().await;
        let active = &app_state.active_game;
        if let Some(info) = app_state.game_info.get(active) {
            let mut s = stream.lock().await;
            protocol::send_message(
                &mut *s,
                &Message::GameInfoExchange {
                    game_info: info.clone(),
                },
            )
            .await?;
        }
    }

    // Request manifest — the host may have sent GameInfoExchange first,
    // so we need to drain it before we get our ManifestResponse.
    let (remote_manifest, host_game_info) = {
        let mut s = stream.lock().await;
        protocol::send_message(&mut *s, &Message::ManifestRequest).await?;

        let mut host_gi: Option<GameInfo> = None;
        loop {
            let msg = protocol::recv_message(&mut *s).await?;
            match msg {
                Message::ManifestResponse { manifest } => break (manifest, host_gi),
                Message::GameInfoExchange { game_info } => {
                    host_gi = Some(sanitize_game_info(game_info));
                }
                Message::Error { message } => return Err(message),
                _ => return Err("Unexpected message while waiting for manifest".to_string()),
            }
        }
    };

    // Send our manifest to the host so it knows our mod count
    {
        let app_state = state.lock().await;
        let manifest = app_state.local_manifest.clone();
        drop(app_state);
        let mut s = stream.lock().await;
        let _ = protocol::send_message(
            &mut *s,
            &Message::ManifestResponse { manifest },
        )
        .await;
    }

    // Store persistent connection BEFORE emitting event so frontend sees peers
    let host_name_for_loop = host_name.clone();
    {
        let mut app_state = state.lock().await;
        let info = crate::state::PeerInfo {
            id: peer_id.to_string(),
            name: host_name,
            ip: ip.to_string(),
            port,
            mod_count: remote_manifest.files.len(),
            version: host_version,
            pin_required: false,
            game_info: host_game_info,
        };
        app_state.connections.insert(
            peer_id.to_string(),
            crate::state::PeerConnection {
                info,
                stream: stream.clone(),
                remote_manifest: Some(remote_manifest),
                sync_plan: None,
                is_syncing: false,
                supports_compression: host_supports_compression,
            },
        );
    }

    // Emit peer-connected AFTER connection is stored so getSessionStatus() returns peers
    let _ = app.emit(
        "peer-connected",
        serde_json::json!({"name": &host_name_for_loop, "peer_id": peer_id}),
    );

    // Client message loop — keeps connection alive, handles host messages,
    // detects disconnects. Runs until the connection drops.
    client_message_loop(state, app, stream, peer_id, host_name_for_loop).await;

    Ok(())
}

/// Client-side message loop: reads messages from the host and detects disconnects.
/// Uses `try_lock` on the stream so sync operations can use the stream concurrently.
/// Runs until the connection drops or the user disconnects. Handles its own cleanup.
async fn client_message_loop(
    state: Arc<Mutex<AppState>>,
    app: tauri::AppHandle,
    stream: Arc<Mutex<TcpStream>>,
    peer_id: &str,
    host_name: String,
) {
    let mut clean_disconnect = false;
    let mut disconnect_reason = String::new();
    let mut last_ping = std::time::Instant::now();

    loop {
        // Check if we're still connected (user may have called disconnect)
        {
            let app_state = state.lock().await;
            if app_state.session_type != crate::state::SessionType::Client
                || !app_state.connections.contains_key(peer_id)
            {
                return; // Already cleaned up externally (e.g. user called disconnect)
            }
        }

        // Try to read from stream without blocking sync operations.
        // try_lock avoids holding the stream while a file transfer is in progress.
        let msg_result = match stream.try_lock() {
            Ok(mut s) => {
                // Send periodic keepalive to prevent host idle timeout
                if last_ping.elapsed() > std::time::Duration::from_secs(20) {
                    if protocol::send_message(&mut *s, &Message::Ping).await.is_ok() {
                        last_ping = std::time::Instant::now();
                    }
                }
                protocol::try_recv_message(&mut *s, std::time::Duration::from_millis(200)).await
            }
            Err(_) => {
                // Stream in use by sync operation (which sends its own messages) — skip
                Ok(None)
            }
        };

        match msg_result {
            Ok(Some(msg)) => match msg {
                Message::Disconnect => {
                    log::info!("Host '{}' sent disconnect", host_name);
                    clean_disconnect = true;
                    break;
                }
                Message::GameInfoExchange { game_info } => {
                    let sanitized = sanitize_game_info(game_info);
                    let mut app_state = state.lock().await;
                    if let Some(conn) = app_state.connections.get_mut(peer_id) {
                        conn.info.game_info = Some(sanitized);
                    }
                    let _ = app.emit(
                        "peer-game-info",
                        serde_json::json!({"peer_id": peer_id}),
                    );
                }
                other => {
                    log::debug!("Client message loop: ignoring {:?}", other);
                }
            },
            Ok(None) => {
                // Timeout or stream busy — sleep before retrying
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            Err(e) => {
                log::warn!("Connection to host '{}' lost: {}", host_name, e);
                disconnect_reason = e;
                break;
            }
        }
    }

    // Clean up connection and reset session state
    {
        let mut app_state = state.lock().await;
        if app_state.connections.remove(peer_id).is_some() {
            app_state.session_type = crate::state::SessionType::None;
            app_state.session_name.clear();
            app_state.local_display_name.clear();
        }
    }

    let reason = if clean_disconnect {
        "Host disconnected".to_string()
    } else if disconnect_reason.is_empty() {
        "Unknown".to_string()
    } else {
        disconnect_reason
    };

    let _ = app.emit(
        "peer-disconnected",
        serde_json::json!({
            "name": &host_name,
            "peer_id": peer_id,
            "clean": clean_disconnect,
            "reason": reason,
        }),
    );
}

/// Request a file from a specific peer over their persistent connection
pub async fn request_file(
    state: &Arc<Mutex<AppState>>,
    peer_id: &str,
    path: &str,
    dest_base: &str,
) -> Result<(), String> {
    let connection = {
        let app_state = state.lock().await;
        app_state
            .connections
            .get(peer_id)
            .map(|c| c.stream.clone())
            .ok_or_else(|| format!("No connection for peer '{}'", peer_id))?
    };

    // Block dangerous file extensions from peers
    if crate::utils::is_dangerous_extension(path) {
        return Err(format!(
            "Blocked dangerous file type: {}",
            path
        ));
    }

    // Validate path stays within base directory before writing
    let dest_path = crate::utils::safe_join(dest_base, path)
        .map_err(|e| format!("Path validation failed for {}: {}", path, e))?;

    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Atomic write: stream chunks to temp file, then rename
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = dest_path.with_extension(
        format!(
            "{}.{}.tmp",
            dest_path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            unique,
        ),
    );

    // Hold stream lock for the entire file transfer to prevent message interleaving
    let expected_hash = {
        let mut s = connection.lock().await;

        // Send file request
        protocol::send_message(&mut *s, &Message::FileRequest { path: path.to_string() }).await?;

        // Receive FileHeader
        let (expected_size, expected_hash) = {
            let msg = protocol::recv_message(&mut *s).await?;
            match msg {
                Message::FileHeader { size, hash, .. } => (size, hash),
                Message::Error { message } => return Err(message),
                _ => return Err("Expected FileHeader".to_string()),
            }
        };

        // Validate file size before writing
        if expected_size > MAX_FILE_SIZE {
            return Err(format!(
                "File too large: {} bytes (max {} bytes)",
                expected_size, MAX_FILE_SIZE
            ));
        }

        // Stream chunks directly to temp file on disk
        let mut out_file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        let mut hasher = Sha256::new();
        let mut bytes_written = 0u64;

        loop {
            let msg = protocol::recv_message(&mut *s).await?;
            match msg {
                Message::FileChunk { data, compressed, .. } => {
                    let raw_decoded = BASE64.decode(&data).map_err(|e| e.to_string())?;
                    let decoded = if compressed {
                        decompress_chunk(&raw_decoded)?
                    } else {
                        raw_decoded
                    };
                    if decoded.len() > MAX_CHUNK_SIZE {
                        return Err(format!(
                            "Chunk too large: {} bytes (max {})",
                            decoded.len(),
                            MAX_CHUNK_SIZE
                        ));
                    }
                    bytes_written += decoded.len() as u64;
                    if bytes_written > expected_size {
                        return Err("Received more data than declared size".to_string());
                    }
                    hasher.update(&decoded);
                    out_file.write_all(&decoded).await.map_err(|e| e.to_string())?;
                }
                Message::FileComplete { .. } => break,
                Message::Error { message } => return Err(message),
                _ => return Err("Unexpected message during file transfer".to_string()),
            }
        }

        out_file.flush().await.map_err(|e| e.to_string())?;

        // Verify hash
        let actual_hash = hex::encode(hasher.finalize());
        if actual_hash != expected_hash {
            // Clean up temp file on hash mismatch
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(format!(
                "Hash mismatch for {}: expected {}, got {}",
                path, expected_hash, actual_hash
            ));
        }

        expected_hash
    };

    // Rename temp file to final destination
    tokio::fs::rename(&tmp_path, &dest_path)
        .await
        .map_err(|e| {
            let tmp = tmp_path.clone();
            tokio::spawn(async move { let _ = tokio::fs::remove_file(tmp).await; });
            e.to_string()
        })?;

    let _ = expected_hash; // suppress unused warning
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        // Simulate a typical mod file chunk (repetitive XML-like content)
        let original = b"<config>\n  <setting name=\"foo\" value=\"bar\" />\n  <setting name=\"baz\" value=\"qux\" />\n</config>\n"
            .repeat(100);

        let compressed = compress_chunk(&original).expect("compression should succeed");
        assert!(compressed.len() < original.len(), "compressed should be smaller than original");

        let decompressed = decompress_chunk(&compressed).expect("decompression should succeed");
        assert_eq!(decompressed, original, "roundtrip should produce identical data");
    }

    #[test]
    fn test_compress_decompress_random_data() {
        // Random-ish data that won't compress well
        let original: Vec<u8> = (0..4096).map(|i| (i * 7 + 13) as u8).collect();

        let compressed = compress_chunk(&original).expect("compression should succeed");
        let decompressed = decompress_chunk(&compressed).expect("decompression should succeed");
        assert_eq!(decompressed, original, "roundtrip should produce identical data");
    }

    #[test]
    fn test_compress_empty() {
        let compressed = compress_chunk(b"").expect("compression of empty data should succeed");
        let decompressed = decompress_chunk(&compressed).expect("decompression should succeed");
        assert_eq!(decompressed, b"");
    }

    #[test]
    fn test_should_skip_compression() {
        assert!(should_skip_compression("Mods/texture.png"));
        assert!(should_skip_compression("Mods/archive.zip"));
        assert!(should_skip_compression("Mods/TEXTURE.PNG")); // case insensitive
        assert!(should_skip_compression("audio/music.mp3"));
        assert!(should_skip_compression("video/intro.mkv"));

        assert!(!should_skip_compression("Mods/mod.package"));
        assert!(!should_skip_compression("config.xml"));
        assert!(!should_skip_compression("script.lua"));
        assert!(!should_skip_compression("data.json"));
    }

    #[test]
    fn test_compression_actually_shrinks_text() {
        // 10KB of repetitive text (typical config/XML file)
        let data = b"key = value\n".repeat(1000);
        let compressed = compress_chunk(&data).expect("compression should succeed");
        let ratio = compressed.len() as f64 / data.len() as f64;
        assert!(ratio < 0.5, "repetitive text should compress to <50%, got {:.0}%", ratio * 100.0);
    }

    #[test]
    fn test_base64_compression_pipeline() {
        // Simulate the full send/receive pipeline: compress → base64 → base64 decode → decompress
        let original = b"<mod name=\"test\">\n  <data>lots of repeated content here</data>\n</mod>\n"
            .repeat(50);

        // Sender side
        let compressed = compress_chunk(&original).expect("compress");
        let b64_encoded = BASE64.encode(&compressed);

        // Receiver side
        let b64_decoded = BASE64.decode(&b64_encoded).expect("base64 decode");
        let decompressed = decompress_chunk(&b64_decoded).expect("decompress");

        assert_eq!(decompressed, original, "full pipeline roundtrip should match");
    }

    #[test]
    fn test_compress_inflation_fallback() {
        // Random bytes that won't compress well — compressed may be larger
        let random_data: Vec<u8> = (0..2048).map(|i| ((i * 251 + 67) % 256) as u8).collect();
        let compressed = compress_chunk(&random_data).expect("compression should succeed");
        // Even if compressed is larger, the roundtrip still works
        let decompressed = decompress_chunk(&compressed).expect("decompression should succeed");
        assert_eq!(decompressed, random_data);
        // The sending logic checks `compressed.len() < n` — verify we can detect inflation
        // (this is just a property test, the actual fallback is in handle_client)
    }

    #[test]
    fn test_multiple_chunks_sequential() {
        // Simulate sending multiple chunks of a file
        let chunks: Vec<Vec<u8>> = (0..5).map(|i| {
            format!("chunk {} data with content {}\n", i, "x".repeat(500)).into_bytes()
        }).collect();

        let mut reconstructed = Vec::new();
        for chunk in &chunks {
            let compressed = compress_chunk(chunk).expect("compress");
            let decompressed = decompress_chunk(&compressed).expect("decompress");
            reconstructed.extend_from_slice(&decompressed);
        }

        let original: Vec<u8> = chunks.into_iter().flatten().collect();
        assert_eq!(reconstructed, original, "sequential chunks should reconstruct correctly");
    }

    #[test]
    fn test_decompress_corrupted_data_returns_error() {
        let garbage = b"this is not valid zstd data";
        let result = decompress_chunk(garbage);
        assert!(result.is_err(), "corrupted data should return an error");
    }

    #[test]
    fn test_compress_large_chunk() {
        // Near MAX_CHUNK_SIZE (1MB)
        let large_data = vec![b'A'; 1_000_000];
        let compressed = compress_chunk(&large_data).expect("compression should succeed");
        assert!(compressed.len() < large_data.len(), "repetitive 1MB should compress well");
        let decompressed = decompress_chunk(&compressed).expect("decompression should succeed");
        assert_eq!(decompressed, large_data);
    }
}
