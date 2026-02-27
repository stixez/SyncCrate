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
    let stream = Arc::new(Mutex::new(stream));

    // Wait for Hello
    let msg = {
        let mut s = stream.lock().await;
        protocol::recv_message(&mut *s).await?
    };

    let (peer_name, peer_pin) = match msg {
        Message::Hello { name, version: _, pin } => {
            // Sanitize: truncate and strip control characters
            let sanitized = name.chars()
                .filter(|c| !c.is_control())
                .take(MAX_PEER_NAME_LEN)
                .collect::<String>();
            (sanitized, pin)
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
            version: String::new(),
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
            let _ = protocol::send_message(
                &mut *s,
                &Message::GameInfoExchange {
                    game_info: info.clone(),
                },
            )
            .await;
        }
    }

    // Handle messages in a loop
    let mut clean_disconnect = false;
    loop {
        let msg = {
            let mut s = stream.lock().await;
            match protocol::recv_message(&mut *s).await {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("Connection lost for peer '{}' ({}): {}", peer_name, peer_id, e);
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
                    filtered.files.retain(|_, info| perms.is_file_allowed(&info.file_type));
                    filtered
                };
                let mut s = stream.lock().await;
                protocol::send_message(&mut *s, &Message::ManifestResponse { manifest }).await?;
            }
            Message::ManifestResponse { manifest } => {
                let mut app_state = state.lock().await;
                if let Some(conn) = app_state.connections.get_mut(&peer_id) {
                    conn.remote_manifest = Some(manifest);
                }
            }
            Message::FileRequest { path } => {
                let base = {
                    let app_state = state.lock().await;

                    // Validate that the requested file is in an allowed folder
                    let allowed = app_state.local_manifest.files.get(&path)
                        .map(|info| app_state.folder_permissions.is_file_allowed(&info.file_type))
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

                        // Second pass: re-read and send chunks
                        // Seek back to start
                        use tokio::io::AsyncSeekExt;
                        file.seek(std::io::SeekFrom::Start(0)).await.map_err(|e| e.to_string())?;

                        loop {
                            let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
                            if n == 0 { break; }
                            protocol::send_message(
                                &mut *s,
                                &Message::FileChunk {
                                    data: BASE64.encode(&buf[..n]),
                                    offset,
                                },
                            )
                            .await?;
                            offset += n as u64;
                        }

                        protocol::send_message(&mut *s, &Message::FileComplete { path }).await?;
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
            },
        )
        .await?;
    }

    // Wait for Welcome (or Error if PIN was rejected)
    let host_name = {
        let mut s = stream.lock().await;
        let msg = protocol::recv_message(&mut *s).await?;
        match msg {
            Message::Welcome { name, .. } => name,
            Message::Error { message } => return Err(message),
            _ => return Err("Expected Welcome message".to_string()),
        }
    };

    let _ = app.emit(
        "peer-connected",
        serde_json::json!({"name": &host_name, "peer_id": peer_id}),
    );

    // Send our game info to the host
    {
        let app_state = state.lock().await;
        let active = &app_state.active_game;
        if let Some(info) = app_state.game_info.get(active) {
            let mut s = stream.lock().await;
            let _ = protocol::send_message(
                &mut *s,
                &Message::GameInfoExchange {
                    game_info: info.clone(),
                },
            )
            .await;
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

    // Store persistent connection in connections map
    {
        let mut app_state = state.lock().await;
        let info = crate::state::PeerInfo {
            id: peer_id.to_string(),
            name: host_name,
            ip: ip.to_string(),
            port,
            mod_count: remote_manifest.files.len(),
            version: String::new(),
            pin_required: false,
            game_info: host_game_info,
        };
        app_state.connections.insert(
            peer_id.to_string(),
            crate::state::PeerConnection {
                info,
                stream,
                remote_manifest: Some(remote_manifest),
                sync_plan: None,
                is_syncing: false,
            },
        );
    }

    Ok(())
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
                Message::FileChunk { data, .. } => {
                    let decoded = BASE64.decode(&data).map_err(|e| e.to_string())?;
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
