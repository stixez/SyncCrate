use crate::event_sink::Events;
use crate::network::protocol::{self, Message};
use crate::state::{AppState, GameInfo};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use crate::network::stream::PeerStream;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Maximum file size we'll accept from a peer (2 GB)
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// A host manifest older than this is rescanned before it's served. The file
/// watcher normally marks changes (empty hashes), but it can miss events or
/// not be running; with the hash cache a rescan mostly just stats files.
const HOST_MANIFEST_MAX_AGE_SECS: u64 = 600;

pub(crate) fn host_manifest_needs_rescan(manifest: &crate::state::FileManifest, now: u64) -> bool {
    manifest.files.is_empty()
        || manifest.files.values().any(|f| f.hash.is_empty())
        || now.saturating_sub(manifest.generated_at) > HOST_MANIFEST_MAX_AGE_SECS
}

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

/// Decompress a zstd-compressed chunk, never to more than `MAX_CHUNK_SIZE`:
/// unbounded `decode_all` let a few MB from a malicious host expand to
/// hundreds of GB in memory (a decompression bomb).
fn decompress_chunk(data: &[u8]) -> Result<Vec<u8>, String> {
    zstd::bulk::decompress(data, MAX_CHUNK_SIZE).map_err(|e| format!("Decompression failed (or chunk over {} bytes): {}", MAX_CHUNK_SIZE, e))
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
            // chars(), not String::truncate: that panics when the cut lands
            // inside a multi-byte character (a crafted pack name killed the
            // handler and left a ghost peer behind).
            p.id.code = clean_peer_text(&p.id.code, MAX_PACK_STRING_LEN);
            p.name = clean_peer_text(&p.name, MAX_PACK_STRING_LEN);
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
/// If the preferred port is taken (another app, or a previous SyncCrate that
/// hasn't released it yet) the next few ports are tried; the port actually
/// bound is returned so it can be advertised and shown to the user.
pub async fn bind_listener(port: u16) -> Result<(TcpListener, u16), String> {
    let mut first_err = None;
    for candidate in port..=port.saturating_add(10) {
        match TcpListener::bind(("0.0.0.0", candidate)).await {
            Ok(l) => {
                if candidate != port {
                    log::warn!("Port {} unavailable, hosting on {} instead", port, candidate);
                }
                return Ok((l, candidate));
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    Err(format!(
        "Failed to bind port {} (and the next 10): {}",
        port,
        first_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Accept loop for an already-bound listener.
pub async fn run_listener(
    listener: TcpListener,
    state: Arc<Mutex<AppState>>,
    app: Events,
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
                tokio::spawn(serve_incoming(PeerStream::tcp(stream), state, app, peer_addr.to_string()));
            }
        }
    }
}

/// Serve one incoming peer (LAN TCP or internet/iroh) until it disconnects.
/// Connections being served at once (handshaking or connected). `MAX_PEERS`
/// only counted peers that had finished the handshake, so thousands of
/// half-open connections could each hold a buffer before the PIN check.
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MAX_IN_FLIGHT: usize = MAX_PEERS + 8;

pub(crate) struct HandshakeSlot;

impl HandshakeSlot {
    pub(crate) fn try_acquire() -> Option<HandshakeSlot> {
        use std::sync::atomic::Ordering;
        if IN_FLIGHT.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(HandshakeSlot)
    }
}

impl Drop for HandshakeSlot {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Removes a peer that `handle_client` added if the handler exits early (an
/// error mid-session or a panic): without it the peer stayed in
/// `connections` forever, and eight of them locked the host.
struct PeerCleanup {
    state: Arc<Mutex<AppState>>,
    app: Events,
    peer_id: String,
    name: String,
    armed: bool,
}

impl Drop for PeerCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (state, app, peer_id, name) = (self.state.clone(), self.app.clone(), self.peer_id.clone(), self.name.clone());
        tokio::spawn(async move {
            if state.lock().await.connections.remove(&peer_id).is_some() {
                let _ = app.emit(
                    "peer-disconnected",
                    serde_json::json!({"name": name, "peer_id": peer_id, "clean": false, "reason": "Connection error"}),
                );
            }
        });
    }
}

/// Strip control and bidi-override characters and cap the length of a
/// string a peer sent that we store or show.
fn clean_peer_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() && !crate::chat::is_bidi_control(*c)).take(max).collect::<String>().trim().to_string()
}

pub async fn serve_incoming(
    stream: PeerStream,
    state: Arc<Mutex<AppState>>,
    app: Events,
    label: String,
) {
    let Some(_slot) = HandshakeSlot::try_acquire() else {
        log::warn!("Rejecting connection from {} — too many connections in progress", label);
        return;
    };
    // Enforce connection limit
    {
        let app_state = state.lock().await;
        if app_state.connections.len() >= MAX_PEERS {
            log::warn!("Rejecting connection from {} — max peers ({}) reached", label, MAX_PEERS);
            // Dropping the stream closes it; the peer sees the connection end
            return;
        }
    }
    if let Err(e) = handle_client(stream, state, app, label).await {
        log::error!("Client handler error: {}", e);
    }
}

async fn handle_client(
    stream: PeerStream,
    state: Arc<Mutex<AppState>>,
    app: Events,
    peer_label: String,
) -> Result<(), String> {
    let peer_ip = stream
        .peer_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| stream.kind().to_string());
    log::info!("Incoming {} connection from {}", stream.kind(), peer_label);
    let stream = Arc::new(Mutex::new(stream));

    // Wait for Hello (size- and time-limited: the peer isn't authenticated yet)
    let msg = {
        let mut s = stream.lock().await;
        protocol::recv_hello(&mut *s).await?
    };

    let use_compression;

    // Only an iroh peer's id is proven (QUIC handshake); over TCP it's a claim.
    let authenticated_node = stream.lock().await.remote_node_id().map(|id| crate::crews::node_id_hex(&id));
    let (peer_name, peer_version, peer_pin, peer_game, peer_node, peer_crews) = match msg {
        Message::Hello { name, version, pin, supports_compression, game_id, node_id, crews, .. } => {
            // Sanitize: truncate and strip control characters
            let sanitized = clean_peer_text(&name, MAX_PEER_NAME_LEN);
            let peer_supports_compression = supports_compression;
            use_compression = peer_supports_compression;
            // A node id claimed over TCP is display data only (see crews docs).
            let node = authenticated_node.clone().or(node_id.filter(|n| crate::crews::is_valid_node_id(n)));
            (sanitized, clean_peer_text(&version, 32), pin, game_id, node, crews)
        }
        _ => return Err("Expected Hello message".to_string()),
    };

    if peer_name.is_empty() {
        return Err("Peer sent empty name".to_string());
    }

    // Validate PIN if the host has one set, with lockouts against guessing
    // (`network::pin_guard`). The source is the proven node id over iroh,
    // the IP over TCP.
    {
        let mut app_state = state.lock().await;
        let source = authenticated_node.clone().unwrap_or_else(|| peer_ip.clone());
        if let Some(expected_pin) = app_state.session_pin.clone() {
            let now = std::time::Instant::now();
            let verdict = match app_state.pin_guard.check(&source, now) {
                Err(wait) => Err(format!("Too many wrong PIN attempts. Try again in {} s.", wait.as_secs().max(1))),
                Ok(()) => match &peer_pin {
                    Some(provided) if crate::network::pin_guard::pin_matches(provided, &expected_pin) => {
                        app_state.pin_guard.record_success(&source);
                        Ok(())
                    }
                    _ => {
                        app_state.pin_guard.record_failure(&source, now);
                        Err("Invalid PIN".to_string())
                    }
                },
            };
            drop(app_state);
            if let Err(message) = verdict {
                let mut s = stream.lock().await;
                protocol::send_message(&mut *s, &Message::Error { message: message.clone() }).await?;
                s.close_gracefully().await;
                return Err(format!("Refused peer {}: {}", source, message));
            }
        }
    }

    // Refuse a client that has a different game selected: its sync plan would
    // compare our files against the wrong game's folder and download them there.
    {
        let host_game = state.lock().await.active_game.clone();
        if protocol::games_conflict(&host_game, peer_game.as_deref()) {
            let mut s = stream.lock().await;
            protocol::send_message(
                &mut *s,
                &Message::Error { message: protocol::wrong_game_error(&host_game) },
            )
            .await?;
            s.close_gracefully().await;
            return Err(format!(
                "Peer '{}' has {} selected, but this session shares {}",
                peer_name,
                peer_game.unwrap_or_default(),
                host_game
            ));
        }
    }

    // Send Welcome. Crew data only after the PIN and game checks above:
    // being in a crew never gets anyone past them. And only to a peer whose
    // identity iroh proved: over TCP anyone can claim any node id and any
    // crew id, which let a stranger read crew data or rename members.
    {
        let mut app_state = state.lock().await;
        let crews = match (&authenticated_node, peer_crews.is_empty()) {
            (Some(node), false) => crate::crews::host_handshake(&mut app_state, &peer_crews, Some((node.clone(), peer_name.clone()))),
            _ => Vec::new(),
        };
        if !crews.is_empty() {
            let _ = app.emit("crews-changed", serde_json::json!({}));
        }
        let our_name = app_state.session_name.clone();
        let our_game = app_state.active_game.clone();
        let our_node = app_state.local_node_id.clone();
        drop(app_state);
        let mut s = stream.lock().await;
        protocol::send_message(
            &mut *s,
            &Message::Welcome {
                name: our_name,
                version: env!("CARGO_PKG_VERSION").to_string(),
                supports_compression: true,
                game_id: Some(our_game),
                node_id: our_node,
                crews,
                features: vec![crate::chat::FEATURE.to_string()],
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
            ip: peer_ip.clone(),
            port: 0,
            mod_count: 0,
            version: peer_version.clone(),
            pin_required: false,
            game_info: None,
            game_id: peer_game.clone(),
            addresses: Vec::new(),
            node_id: peer_node.clone(),
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
        let now = crate::utils::timestamp_now();
        if app_state.chat.post(&peer_name, &format!("{} joined", crate::chat::clean_name(&peer_name)), true, now).is_some() {
            let _ = app.emit("chat-updated", serde_json::json!({}));
        }
    }
    let mut cleanup = PeerCleanup { state: state.clone(), app: app.clone(), peer_id: peer_id.clone(), name: peer_name.clone(), armed: true };

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

    // Handle messages in a loop (5-minute idle timeout; TCP keepalive detects dead connections).
    // Poll in short slices and release the stream lock between them: holding it for
    // the whole idle wait blocked `disconnect`/`disconnect_peer` (which need the lock
    // to send `Disconnect`) until the client's next ping arrived.
    const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
    let mut clean_disconnect = false;
    let mut removed_externally = false;
    let mut disconnect_reason = String::new();
    let mut peer_files_sent: u64 = 0;
    let mut chat_limiter = crate::chat::RateLimiter::default();
    let speed_limit = crate::commands::sync::get_speed_limit();
    loop {
        let idle_since = std::time::Instant::now();
        let msg = loop {
            {
                let app_state = state.lock().await;
                if !app_state.connections.contains_key(&peer_id) {
                    removed_externally = true;
                    break None;
                }
            }
            let polled = {
                let mut s = stream.lock().await;
                protocol::try_recv_message(&mut *s, std::time::Duration::from_millis(250)).await
            };
            match polled {
                Ok(Some(m)) => break Some(m),
                Ok(None) => {
                    if idle_since.elapsed() >= IDLE_TIMEOUT {
                        log::warn!("Idle timeout for peer '{}' ({})", peer_name, peer_id);
                        disconnect_reason = "Idle timeout (5 min)".to_string();
                        break None;
                    }
                    // Give other lock holders (disconnect, etc.) a chance.
                    tokio::task::yield_now().await;
                }
                Err(e) => {
                    log::warn!("Connection lost for peer '{}' ({}): {}", peer_name, peer_id, e);
                    disconnect_reason = e;
                    break None;
                }
            }
        };
        let Some(msg) = msg else { break };

        match msg {
            Message::ManifestRequest => {
                // Rescan (hashed) when the manifest is empty, has quick-scan
                // empty hashes, or is old. Serving an empty/stale manifest made
                // clients report "in sync" or fail with "File not available".
                let needs_rehash = {
                    let app_state = state.lock().await;
                    host_manifest_needs_rescan(&app_state.local_manifest, crate::utils::timestamp_now())
                };

                // Re-scan with full hashes if needed so the peer gets accurate data
                if needs_rehash {
                    log::info!("Re-scanning with hashes before sending manifest to peer '{}'", peer_name);
                    if let Err(e) = crate::commands::files::scan_files_inner(&state, None, true).await {
                        log::warn!("Failed to re-scan with hashes: {}", e);
                    }
                }

                let manifest = {
                    let app_state = state.lock().await;
                    let mut filtered = app_state.local_manifest.clone();
                    filtered.files.retain(|_, info| app_state.is_file_info_allowed(info));
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
                        .map(|info| app_state.is_file_info_allowed(info))
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
                        // Get file size + mtime for the header / cache check
                        let metadata = file.metadata().await.map_err(|e| e.to_string())?;
                        let file_size = metadata.len();
                        let file_mtime = metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        // Determine if compression should be used for this file
                        let compress_this_file = use_compression
                            && file_size >= MIN_COMPRESS_SIZE
                            && !should_skip_compression(&path);

                        let chunk_size = 65536;
                        let mut buf = vec![0u8; chunk_size];
                        let mut offset = 0u64;

                        // Reuse the hash computed during scanning when the file is
                        // unchanged (same size + mtime). Re-hashing the whole file on
                        // every request meant a second full read; for large files that
                        // could exceed the requester's read timeout and abort the sync.
                        let cached_hash = {
                            let app_state = state.lock().await;
                            app_state.local_manifest.files.get(&path).and_then(|info| {
                                if info.size == file_size
                                    && info.modified == file_mtime
                                    && !info.hash.is_empty()
                                {
                                    Some(info.hash.clone())
                                } else {
                                    None
                                }
                            })
                        };

                        let hash = match cached_hash {
                            Some(h) => h,
                            None => {
                                // Cache miss/stale: compute by streaming once, then rewind.
                                use tokio::io::AsyncSeekExt;
                                let mut hasher = Sha256::new();
                                loop {
                                    let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
                                    if n == 0 { break; }
                                    hasher.update(&buf[..n]);
                                }
                                file.seek(std::io::SeekFrom::Start(0)).await.map_err(|e| e.to_string())?;
                                hex::encode(hasher.finalize())
                            }
                        };

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

                        // Send file content (file is positioned at the start: either
                        // freshly opened with a cached hash, or rewound after hashing).
                        // Throttling: track cumulative bytes since file start.
                        // The expected/elapsed comparison handles multi-second windows
                        // without needing periodic resets.
                        let mut throttle_bytes = 0u64;
                        let throttle_start = tokio::time::Instant::now();

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

                            // Apply bandwidth throttle (stream lock is held intentionally
                            // for the entire file send to prevent chunk interleaving)
                            if speed_limit > 0 {
                                throttle_bytes += n as u64;
                                let elapsed = throttle_start.elapsed();
                                let expected = std::time::Duration::from_secs_f64(throttle_bytes as f64 / speed_limit as f64);
                                if expected > elapsed {
                                    tokio::time::sleep(expected - elapsed).await;
                                }
                            }

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
            Message::ChatSync { since, outgoing, synced_files } => {
                let (reply, changed, accepted) = {
                    let mut app_state = state.lock().await;
                    let now = crate::utils::timestamp_now();
                    crate::chat::host_sync(&mut app_state.chat, &mut chat_limiter, &peer_name, since, outgoing, synced_files, now)
                };
                if changed {
                    let _ = app.emit("chat-updated", serde_json::json!({}));
                }
                let mut s = stream.lock().await;
                protocol::send_message(&mut *s, &Message::ChatBatch { messages: reply, accepted: Some(accepted) }).await?;
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

    {
        let mut app_state = state.lock().await;
        let now = crate::utils::timestamp_now();
        if app_state.session_type == crate::state::SessionType::Host
            && app_state.chat.post(&peer_name, &format!("{} left", crate::chat::clean_name(&peer_name)), true, now).is_some()
        {
            let _ = app.emit("chat-updated", serde_json::json!({}));
        }
    }

    // The normal exit does its own cleanup (and emits the event) below.
    cleanup.armed = false;

    // disconnect / disconnect_peer already removed the peer and emitted the event.
    if removed_externally {
        return Ok(());
    }

    // Emit disconnect event (whether clean or unexpected)
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

/// Why a single TCP connect attempt failed — used to pick the most helpful message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ConnectFailure {
    // Ordered from least to most informative; the "best" failure wins the report.
    Timeout,
    Unreachable,
    Other,
    Refused,
}

fn classify_connect_error(e: &std::io::Error) -> ConnectFailure {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::ConnectionRefused => ConnectFailure::Refused,
        ErrorKind::TimedOut => ConnectFailure::Timeout,
        _ => {
            // HostUnreachable / NetworkUnreachable are unstable ErrorKinds on
            // older toolchains; match on the OS codes instead
            // (Windows WSAENETUNREACH 10051, WSAEHOSTUNREACH 10065; Unix 101/113).
            match e.raw_os_error() {
                Some(10051) | Some(10065) | Some(101) | Some(113) => ConnectFailure::Unreachable,
                Some(10060) | Some(110) => ConnectFailure::Timeout,
                _ => ConnectFailure::Other,
            }
        }
    }
}

fn connect_failure_message(kind: ConnectFailure, tried: &[String], port: u16, detail: &str) -> String {
    let addrs = tried.join(", ");
    match kind {
        ConnectFailure::Refused => format!(
            "The host at {} refused the connection on port {}. It's reachable, but SyncCrate isn't hosting on that port — check the host is still hosting and the port matches.",
            addrs, port
        ),
        ConnectFailure::Timeout => format!(
            "No response from {} on port {}. This is almost always Windows Firewall on the host blocking SyncCrate — on the host PC, click \"Fix Windows Firewall\" (shown on the hosting screen and in Network Check), then try again.",
            addrs, port
        ),
        ConnectFailure::Unreachable => format!(
            "{} is unreachable from this PC. Make sure both PCs are on the same network (or the same VPN such as Tailscale/ZeroTier).",
            addrs
        ),
        ConnectFailure::Other => format!("Could not connect to {}:{} — {}", addrs, port, detail),
    }
}

/// Connect to the first reachable address. Attempts are staggered (a light
/// "happy eyeballs"): the best-ranked address gets a head start, then the
/// others are tried in parallel so one dead virtual-adapter address can't
/// eat the whole timeout.
async fn connect_any(addresses: &[String], port: u16) -> Result<TcpStream, String> {
    let mut targets: Vec<std::net::SocketAddr> = Vec::new();
    for a in addresses {
        if let Ok(ip) = a.trim().trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>() {
            let sa = std::net::SocketAddr::new(ip, port);
            if !targets.contains(&sa) {
                targets.push(sa);
            }
        }
    }
    if targets.is_empty() {
        return Err("No valid address for this host".to_string());
    }

    let mut set = tokio::task::JoinSet::new();
    for (i, target) in targets.iter().copied().enumerate() {
        set.spawn(async move {
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(400 * i as u64)).await;
            }
            match tokio::time::timeout(std::time::Duration::from_secs(8), TcpStream::connect(target)).await {
                Ok(Ok(s)) => Ok(s),
                Ok(Err(e)) => Err((classify_connect_error(&e), e.to_string())),
                Err(_) => Err((ConnectFailure::Timeout, "timed out".to_string())),
            }
        });
    }

    let mut best: Option<(ConnectFailure, String)> = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(stream)) => {
                set.abort_all();
                return Ok(stream);
            }
            Ok(Err((kind, detail))) => {
                if best.as_ref().map_or(true, |(k, _)| kind > *k) {
                    best = Some((kind, detail));
                }
            }
            Err(_) => {}
        }
    }

    let tried: Vec<String> = targets.iter().map(|t| t.ip().to_string()).collect();
    let (kind, detail) = best.unwrap_or((ConnectFailure::Other, "unknown error".to_string()));
    Err(connect_failure_message(kind, &tried, port, &detail))
}

/// Connect over the LAN and/or the internet. With both available they race:
/// LAN gets a short head start (it's faster when it works), and whichever
/// connects first wins. If both fail, both reasons are reported.
async fn connect_best(
    addresses: &[String],
    port: u16,
    internet_id: Option<iroh::EndpointId>,
    state: &Arc<Mutex<AppState>>,
    app: &Events,
) -> Result<PeerStream, String> {
    let Some(remote) = internet_id else {
        return connect_any(addresses, port).await.map(PeerStream::tcp);
    };
    if addresses.is_empty() {
        return crate::network::iroh_net::connect(state, app, remote).await;
    }

    let lan = async { connect_any(addresses, port).await.map(PeerStream::tcp) };
    let internet = async {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        crate::network::iroh_net::connect(state, app, remote).await
    };
    tokio::pin!(lan);
    tokio::pin!(internet);

    tokio::select! {
        r = &mut lan => match r {
            Ok(s) => Ok(s),
            Err(lan_err) => internet.await.map_err(|net_err| format!("{}\n\nInternet: {}", lan_err, net_err)),
        },
        r = &mut internet => match r {
            Ok(s) => Ok(s),
            Err(net_err) => lan.await.map_err(|lan_err| format!("{}\n\nInternet: {}", lan_err, net_err)),
        },
    }
}

pub async fn connect_to_host(
    addresses: &[String],
    port: u16,
    internet_id: Option<iroh::EndpointId>,
    peer_id: &str,
    state: Arc<Mutex<AppState>>,
    app: Events,
    pin: Option<String>,
) -> Result<(), String> {
    let stream = connect_best(addresses, port, internet_id, &state, &app).await?;
    run_client_session(stream, addresses, port, peer_id, state, app, pin).await
}

/// Everything after the transport connects: the handshake, manifest exchange
/// and message loop. Transport-neutral (`PeerStream` covers TCP and iroh), so
/// tests can drive it directly with a hand-built `PeerStream` — e.g. a raw
/// iroh connection with relays disabled — without going through
/// `connect_best`/`iroh_net`'s app-wide singleton endpoint. `addresses`/`port`
/// are only recorded on the stored `PeerInfo` (for reconnects); pass `&[]`/`0`
/// for a stream that didn't come from an address (e.g. a test iroh dial).
pub(crate) async fn run_client_session(
    stream: PeerStream,
    addresses: &[String],
    port: u16,
    peer_id: &str,
    state: Arc<Mutex<AppState>>,
    app: Events,
    pin: Option<String>,
) -> Result<(), String> {
    let ip = stream
        .peer_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| stream.kind().to_string());
    let ip = ip.as_str();

    // Dialled over iroh, the host's id is proven by the connection itself.
    let dialled_node = stream.remote_node_id().map(|id| crate::crews::node_id_hex(&id));
    let stream = Arc::new(Mutex::new(stream));

    // Send Hello with our display name (not the session/peer name)
    {
        let app_state = state.lock().await;
        let our_name = app_state.local_display_name.clone();
        let our_game = app_state.active_game.clone();
        let our_node = app_state.local_node_id.clone();
        // Crew ids only to a host iroh proved is a member of that crew: sent
        // to any host, they leaked to spoofed LAN hosts and random join codes.
        let crews = dialled_node.as_deref().map(|h| crate::crews::hellos_for_host(&app_state, h)).unwrap_or_default();
        drop(app_state);
        let mut s = stream.lock().await;
        protocol::send_message(
            &mut *s,
            &Message::Hello {
                name: our_name,
                version: env!("CARGO_PKG_VERSION").to_string(),
                pin: pin.clone(),
                supports_compression: true,
                game_id: Some(our_game),
                node_id: our_node,
                crews,
                features: vec![crate::chat::FEATURE.to_string()],
            },
        )
        .await?;
    }

    // Wait for Welcome (or Error if PIN was rejected)
    let (host_name, host_version, _host_supports_compression, host_game, host_node, host_chat) = {
        let mut s = stream.lock().await;
        let msg = protocol::recv_message(&mut *s).await?;
        match msg {
            Message::Welcome { name, version, supports_compression, game_id, node_id, crews, features } => {
                // Check on our side too, in case the host skipped it.
                let ours = state.lock().await.active_game.clone();
                if protocol::games_conflict(&ours, game_id.as_deref()) {
                    let _ = protocol::send_message(&mut *s, &Message::Disconnect).await;
                    return Err(protocol::wrong_game_error(game_id.as_deref().unwrap_or_default()));
                }
                let host_node = dialled_node.clone().or(node_id.filter(|n| crate::crews::is_valid_node_id(n)));
                let name = clean_peer_text(&name, MAX_PEER_NAME_LEN);
                // Crew data only from a host iroh proved, and only for crews
                // it's a member of (checked in client_handshake).
                if let (Some(proven), false) = (&dialled_node, crews.is_empty()) {
                    let host = crate::crews::CrewHost {
                        node_id: proven.clone(),
                        name: crate::crews::clean_name(&name).unwrap_or_else(|| "Host".into()),
                        addresses: addresses.to_vec(),
                        port,
                        game_id: ours.clone(),
                        at: crate::utils::timestamp_now(),
                    };
                    let mut app_state = state.lock().await;
                    if crate::crews::client_handshake(&mut app_state, crews, host) {
                        let _ = app.emit("crews-changed", serde_json::json!({}));
                    }
                }
                (name, clean_peer_text(&version, 32), supports_compression, game_id, host_node, crate::chat::supports(&features))
            }
            // Shown to the user: a host must not be able to paste a novel into the UI.
            Message::Error { message } => return Err(clean_peer_text(&message, 300)),
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
            // The host may hash a large content folder before replying, which can
            // take well over the default per-message timeout. Wait generously here
            // so a slow first scan doesn't drop the connection mid-handshake.
            let msg = match protocol::try_recv_message(&mut *s, std::time::Duration::from_secs(300)).await? {
                Some(m) => m,
                None => return Err(
                    "Timed out waiting for host manifest — the host may be scanning a very large folder. Try again in a moment.".to_string()
                ),
            };
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
    // Re-scan with hashes if the local manifest only has empty hashes (from quick scan)
    {
        let needs_rehash = {
            let app_state = state.lock().await;
            app_state.local_manifest.files.values().any(|f| f.hash.is_empty())
        };
        if needs_rehash {
            log::info!("Re-scanning with hashes before sending manifest to host");
            let _ = crate::commands::files::scan_files_inner(&state, None, true).await;
        }

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
        if !crate::commands::session::client_attempt_is_active(&app_state, peer_id) {
            return Err("Connection attempt was cancelled or replaced".to_string());
        }

        let info = crate::state::PeerInfo {
            id: peer_id.to_string(),
            name: host_name,
            ip: ip.to_string(),
            port,
            mod_count: remote_manifest.files.len(),
            version: host_version,
            pin_required: false,
            game_info: host_game_info,
            game_id: host_game,
            addresses: addresses.to_vec(),
            node_id: host_node,
        };
        app_state.connections.insert(
            peer_id.to_string(),
            crate::state::PeerConnection {
                info,
                stream: stream.clone(),
                remote_manifest: Some(remote_manifest),
                sync_plan: None,
                is_syncing: false,
            },
        );
        app_state.pending_client_peer_id = None;
        app_state.chat.clear();
        app_state.chat.available = host_chat;
    }

    // Emit peer-connected AFTER connection is stored so getSessionStatus() returns peers
    let _ = app.emit(
        "peer-connected",
        serde_json::json!({"name": &host_name_for_loop, "peer_id": peer_id}),
    );

    // Client message loop — keeps connection alive, handles host messages,
    // detects disconnects. Runs until the connection drops.
    client_message_loop(state, app, stream, peer_id, host_name_for_loop, host_chat).await;

    Ok(())
}

/// Client-side message loop: reads messages from the host and detects disconnects.
/// Uses `try_lock` on the stream so sync operations can use the stream concurrently.
/// Runs until the connection drops or the user disconnects. Handles its own cleanup.
async fn client_message_loop(
    state: Arc<Mutex<AppState>>,
    app: Events,
    stream: Arc<Mutex<PeerStream>>,
    peer_id: &str,
    host_name: String,
    chat: bool,
) {
    let mut clean_disconnect = false;
    let mut disconnect_reason = String::new();
    let mut last_ping = std::time::Instant::now();
    let mut last_chat = std::time::Instant::now() - CHAT_POLL;

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

        // Chat poll inputs are read before taking the stream lock and applied
        // after releasing it: elsewhere the AppState lock is taken first, so
        // taking it while holding the stream could deadlock.
        let mut chat_req = if chat {
            let st = state.lock().await;
            let due = last_chat.elapsed() >= CHAT_POLL || !st.chat.outbox.is_empty() || st.chat.pending_synced.is_some();
            due.then(|| {
                let out: Vec<String> = st.chat.outbox.iter().take(crate::chat::MAX_OUTGOING).cloned().collect();
                (st.chat.last_seq(), out, st.chat.pending_synced)
            })
        } else {
            None
        };
        let mut chat_reply: Option<((Vec<crate::chat::ChatMessage>, Option<usize>), usize, bool)> = None;

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
                match chat_req.take() {
                    Some((since, outgoing, synced)) => {
                        last_chat = std::time::Instant::now();
                        let (sent, reported) = (outgoing.len(), synced.is_some());
                        match chat_round_trip(&mut s, since, outgoing, synced).await {
                            Ok((batch, pending)) => {
                                if let Some(batch) = batch {
                                    chat_reply = Some((batch, sent, reported));
                                }
                                match pending {
                                    Some(m) => Ok(Some(m)),
                                    None => protocol::try_recv_message(&mut *s, std::time::Duration::from_millis(200)).await,
                                }
                            }
                            Err(e) => Err(e),
                        }
                    }
                    None => protocol::try_recv_message(&mut *s, std::time::Duration::from_millis(200)).await,
                }
            }
            Err(_) => {
                // Stream in use by sync operation (which sends its own messages) — skip
                Ok(None)
            }
        };

        if let Some((batch, sent, reported)) = chat_reply {
            let (batch, accepted) = batch;
            // Lines past the host's rate limit weren't posted: keep them
            // queued for the next poll instead of dropping them silently.
            let sent = accepted.map_or(sent, |a| a.min(sent));
            let mut st = state.lock().await;
            let n = sent.min(st.chat.outbox.len());
            st.chat.outbox.drain(..n);
            if reported {
                st.chat.pending_synced = None;
            }
            if st.chat.merge_batch(batch) || n > 0 {
                let _ = app.emit("chat-updated", serde_json::json!({}));
            }
        }

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
            app_state.pending_client_peer_id = None;
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

/// How often an idle client asks the host for new chat lines.
const CHAT_POLL: std::time::Duration = std::time::Duration::from_millis(1000);

/// One chat poll (see `crate::chat`), with the stream lock held by the
/// caller so the host's reply can't reach a sync reader. Returns the batch
/// (None if the host disconnected first) and a message the loop must still
/// act on (the host's `Disconnect` or `GameInfoExchange`).
async fn chat_round_trip(
    s: &mut PeerStream,
    since: u64,
    outgoing: Vec<String>,
    synced_files: Option<u64>,
) -> Result<(Option<(Vec<crate::chat::ChatMessage>, Option<usize>)>, Option<Message>), String> {
    protocol::send_message(s, &Message::ChatSync { since, outgoing, synced_files }).await?;
    let mut pending = None;
    loop {
        let msg = protocol::try_recv_message(s, std::time::Duration::from_secs(15))
            .await?
            .ok_or_else(|| "The host stopped answering".to_string())?;
        match msg {
            Message::ChatBatch { messages, accepted } => return Ok((Some((messages, accepted)), pending)),
            Message::Disconnect => return Ok((None, Some(Message::Disconnect))),
            m @ Message::GameInfoExchange { .. } => pending = Some(m),
            other => log::debug!("Chat poll: ignoring {:?}", other),
        }
    }
}

/// Client side: re-request the host's manifest over the existing connection and
/// store it on the peer connection. Holds the stream lock for the whole
/// request/response (like `request_file`) so `client_message_loop`'s `try_lock`
/// reads can't steal the response.
pub async fn refresh_remote_manifest(
    state: &Arc<Mutex<AppState>>,
    peer_id: &str,
) -> Result<crate::state::FileManifest, String> {
    let stream = {
        let app_state = state.lock().await;
        app_state
            .connections
            .get(peer_id)
            .map(|c| c.stream.clone())
            .ok_or_else(|| format!("No connection for peer '{}'", peer_id))?
    };

    let manifest = {
        let mut s = stream.lock().await;
        protocol::send_message(&mut *s, &Message::ManifestRequest).await?;
        loop {
            // The host may re-hash changed files before replying.
            let msg = match protocol::try_recv_message(&mut *s, std::time::Duration::from_secs(120)).await? {
                Some(m) => m,
                None => return Err("Timed out waiting for host manifest".to_string()),
            };
            match msg {
                Message::ManifestResponse { manifest } => break manifest,
                Message::GameInfoExchange { game_info } => {
                    let sanitized = sanitize_game_info(game_info);
                    let mut app_state = state.lock().await;
                    if let Some(conn) = app_state.connections.get_mut(peer_id) {
                        conn.info.game_info = Some(sanitized);
                    }
                }
                Message::Error { message } => return Err(message),
                // The host closes the socket right after this; the message loop's
                // next read fails and runs the normal disconnect cleanup.
                Message::Disconnect => return Err("Host disconnected".to_string()),
                _ => {} // Pong etc.
            }
        }
    };

    let mut app_state = state.lock().await;
    let conn = app_state
        .connections
        .get_mut(peer_id)
        .ok_or("Peer disconnected")?;
    conn.info.mod_count = manifest.files.len();
    conn.remote_manifest = Some(manifest.clone());
    Ok(manifest)
}

/// What a download may do to a file already at its destination.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplacePolicy {
    /// Plain receive / keep-both: never overwrite a local file with different
    /// content. (Real bug: a case-variant path diffed as "new" and the rename
    /// silently replaced the user's own file.)
    MustNotExist,
    /// "Use theirs": replace only while the local file still has this hash,
    /// i.e. it wasn't edited between the compare and the sync.
    ReplaceIfHash(String),
}

/// One planned download.
pub struct ReceiveRequest<'a> {
    /// Path the host serves the file under.
    pub remote_path: &'a str,
    /// Path to write locally (differs for keep-both and for "use theirs" on a
    /// case-variant or disabled local copy).
    pub local_path: &'a str,
    /// Hash from the manifest the plan was built from (empty = don't check).
    pub expected_hash: &'a str,
    /// Size from that manifest: the host's FileHeader must agree, or a host
    /// could fill the disk with files the plan showed as tiny.
    pub expected_size: Option<u64>,
    pub policy: ReplacePolicy,
}

pub const LOCAL_COPY_DIFFERS: &str =
    "Conflict: a different local copy already exists, so yours was kept. Compare again to resolve it";
pub const LOCAL_CHANGED: &str = "Skipped: your copy changed since the compare. Compare again";
pub const HOST_CHANGED: &str = "Skipped: the host changed this file since the compare. Compare again";

/// Outcome of checking the destination before downloading.
#[derive(Debug, PartialEq)]
pub(crate) enum PreCheck {
    Download,
    /// Already there with the expected content (e.g. completed by an
    /// interrupted earlier attempt): nothing to transfer.
    AlreadyPresent,
    Refuse(&'static str),
}

pub(crate) fn precheck(existing_hash: Option<&str>, expected_hash: &str, policy: &ReplacePolicy) -> PreCheck {
    match existing_hash {
        None => PreCheck::Download,
        Some(h) if !expected_hash.is_empty() && h == expected_hash => PreCheck::AlreadyPresent,
        Some(h) => match policy {
            ReplacePolicy::MustNotExist => PreCheck::Refuse(LOCAL_COPY_DIFFERS),
            ReplacePolicy::ReplaceIfHash(want) if !want.is_empty() && h == want => PreCheck::Download,
            ReplacePolicy::ReplaceIfHash(_) => PreCheck::Refuse(LOCAL_CHANGED),
        },
    }
}

/// Name comparison the way Windows sees it: case-insensitive, trailing dots
/// and spaces ignored.
fn same_file_name(a: &str, b: &str) -> bool {
    a.trim_end_matches(['.', ' ']).to_lowercase() == b.trim_end_matches(['.', ' ']).to_lowercase()
}

/// The file already occupying `dest`, matched case-insensitively among its
/// siblings so behavior is the same on case-sensitive file systems.
pub(crate) fn find_existing(dest: &std::path::Path) -> Option<std::path::PathBuf> {
    if std::fs::symlink_metadata(dest).is_ok() {
        return Some(dest.to_path_buf());
    }
    let name = dest.file_name()?.to_str()?;
    std::fs::read_dir(dest.parent()?)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_str().map_or(false, |n| same_file_name(n, name)))
        .map(|e| e.path())
}

struct ExistingFile {
    path: std::path::PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
    hash: String,
}

fn hash_file_sync(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::with_capacity(131072, file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 131072];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn inspect_existing(dest: &std::path::Path) -> Result<Option<ExistingFile>, String> {
    let Some(path) = find_existing(dest) else { return Ok(None) };
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("Refusing to replace a link or folder at {}", path.display()));
    }
    let hash = hash_file_sync(&path)?;
    Ok(Some(ExistingFile { len: meta.len(), modified: meta.modified().ok(), path, hash }))
}

/// Whether the destination is still as it was before the download started.
fn destination_unchanged(dest: &std::path::Path, before: Option<&ExistingFile>) -> bool {
    match before {
        None => find_existing(dest).is_none(),
        Some(e) => std::fs::symlink_metadata(&e.path)
            .map(|m| m.is_file() && m.len() == e.len && m.modified().ok() == e.modified)
            .unwrap_or(false),
    }
}

/// Download one planned file from the host over the persistent connection.
///
/// Safety checks (each one a real or audited data-loss path):
/// - an existing local file is only replaced per `policy` (never for a plain
///   receive), and is re-checked right before the final rename;
/// - the host's `FileHeader` hash must match the plan's hash, so a file the
///   host changed after the compare isn't written unseen;
/// - a destination that already has the expected content is left alone.
pub async fn receive_file(
    state: &Arc<Mutex<AppState>>,
    peer_id: &str,
    dest_base: &str,
    req: ReceiveRequest<'_>,
) -> Result<bool, String> {
    let connection = {
        let app_state = state.lock().await;
        app_state
            .connections
            .get(peer_id)
            .map(|c| c.stream.clone())
            .ok_or_else(|| format!("No connection for peer '{}'", peer_id))?
    };

    // Block dangerous file extensions from peers
    for p in [req.remote_path, req.local_path] {
        if crate::utils::is_dangerous_extension(p) {
            return Err(format!("Blocked dangerous file type: {}", p));
        }
    }

    // Validate path stays within base directory before writing
    let mut dest_path = crate::utils::safe_join(dest_base, req.local_path)
        .map_err(|e| format!("Path validation failed for {}: {}", req.local_path, e))?;

    let existing = {
        let d = dest_path.clone();
        tokio::task::spawn_blocking(move || inspect_existing(&d))
            .await
            .map_err(|e| e.to_string())??
    };
    match precheck(existing.as_ref().map(|e| e.hash.as_str()), req.expected_hash, &req.policy) {
        PreCheck::AlreadyPresent => return Ok(false),
        PreCheck::Refuse(msg) => return Err(msg.to_string()),
        PreCheck::Download => {}
    }
    // Replace the existing file under its own name: its case may differ from
    // the plan's path, and on a case-sensitive disk we'd otherwise add a twin.
    if let Some(e) = &existing {
        dest_path = e.path.clone();
    }

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
    let mut s = connection.lock().await;

    // Send file request
    protocol::send_message(&mut *s, &Message::FileRequest { path: req.remote_path.to_string() }).await?;

    // Receive FileHeader
    let (expected_size, header_hash) = {
        let msg = protocol::recv_message(&mut *s).await?;
        match msg {
            Message::FileHeader { size, hash, .. } => (size, hash),
            Message::Error { message } => return Err(message),
            _ => return Err("Expected FileHeader".to_string()),
        }
    };

    if req.expected_size.is_some_and(|want| want != expected_size) {
        drain_until_file_end(&mut s).await;
        return Err(format!("Host sent {} bytes for {}, but its file list said {}", expected_size, req.remote_path, req.expected_size.unwrap_or(0)));
    }
    if !req.expected_hash.is_empty() && header_hash != req.expected_hash {
        // The host streams the body regardless; consume it so the next
        // request doesn't read stale chunks.
        drain_until_file_end(&mut s).await;
        return Err(HOST_CHANGED.to_string());
    }

    // From here on the host streams chunks until FileComplete. If we bail out
    // early, drain the rest so the next request doesn't read stale chunks
    // ("Expected FileHeader"), and always remove the temp file.
    let result = receive_file_body(&mut s, &tmp_path, expected_size, &header_hash, req.remote_path).await;
    if let Err((e, stream_in_sync)) = result {
        if !stream_in_sync {
            drain_until_file_end(&mut s).await;
        }
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e);
    }
    drop(s);

    // The user (or the game) may have touched the destination during the download.
    let unchanged = {
        let d = dest_path.clone();
        tokio::task::spawn_blocking(move || destination_unchanged(&d, existing.as_ref()))
            .await
            .unwrap_or(false)
    };
    if !unchanged {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(match req.policy {
            ReplacePolicy::MustNotExist => LOCAL_COPY_DIFFERS.to_string(),
            ReplacePolicy::ReplaceIfHash(_) => LOCAL_CHANGED.to_string(),
        });
    }

    // Rename temp file to final destination
    if let Err(e) = tokio::fs::rename(&tmp_path, &dest_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e.to_string());
    }
    Ok(true)
}

/// Receive chunks into `tmp_path` and verify size + hash.
/// On error returns `(message, stream_in_sync)`; `stream_in_sync` is true when
/// the host's transfer has already ended (FileComplete / Error was consumed).
/// The temp file handle is always closed before returning so it can be deleted
/// on Windows.
async fn receive_file_body(
    s: &mut PeerStream,
    tmp_path: &std::path::Path,
    expected_size: u64,
    expected_hash: &str,
    path: &str,
) -> Result<(), (String, bool)> {
    if expected_size > MAX_FILE_SIZE {
        return Err((format!("File too large: {} bytes (max {} bytes)", expected_size, MAX_FILE_SIZE), false));
    }

    let mut out_file = tokio::fs::File::create(tmp_path)
        .await
        .map_err(|e| (format!("Failed to create temp file: {}", e), false))?;
    let mut hasher = Sha256::new();
    let mut bytes_written = 0u64;

    loop {
        let msg = protocol::recv_message(s).await.map_err(|e| (e, false))?;
        match msg {
            Message::FileChunk { data, compressed, .. } => {
                let raw_decoded = BASE64.decode(&data).map_err(|e| (e.to_string(), false))?;
                let decoded = if compressed {
                    decompress_chunk(&raw_decoded).map_err(|e| (e, false))?
                } else {
                    raw_decoded
                };
                if decoded.len() > MAX_CHUNK_SIZE {
                    return Err((format!("Chunk too large: {} bytes (max {})", decoded.len(), MAX_CHUNK_SIZE), false));
                }
                // Empty chunks make no progress; accepting them let a host
                // hold the stream (and the sync) forever.
                if decoded.is_empty() {
                    return Err(("Host sent an empty chunk".to_string(), false));
                }
                bytes_written += decoded.len() as u64;
                if bytes_written > expected_size {
                    return Err(("Received more data than declared size".to_string(), false));
                }
                hasher.update(&decoded);
                out_file.write_all(&decoded).await.map_err(|e| (e.to_string(), false))?;
            }
            Message::FileComplete { .. } => break,
            Message::Error { message } => return Err((message, true)),
            _ => return Err(("Unexpected message during file transfer".to_string(), false)),
        }
    }

    out_file.flush().await.map_err(|e| (e.to_string(), true))?;
    drop(out_file);

    let actual_hash = hex::encode(hasher.finalize());
    if actual_hash != expected_hash {
        return Err((format!("Hash mismatch for {}: expected {}, got {}", path, expected_hash, actual_hash), true));
    }
    Ok(())
}

/// Discard messages until the host finishes the current file. Gives up after
/// a bounded number of messages / a read error (the connection is then
/// unusable anyway and the loop will notice).
async fn drain_until_file_end(s: &mut PeerStream) {
    // A 2 GB file at 64 KB per chunk is ~32k chunks.
    for _ in 0..40_000 {
        match protocol::recv_message(s).await {
            Ok(Message::FileComplete { .. }) | Ok(Message::Error { .. }) => return,
            Ok(_) => continue,
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompression_is_bounded() {
        // 64 MB of zeros compresses to a few KB; it must not expand past a chunk.
        let bomb = zstd::encode_all(std::io::Cursor::new(vec![0u8; 64 * 1024 * 1024]), 3).unwrap();
        assert!(bomb.len() < 1024 * 1024);
        assert!(decompress_chunk(&bomb).is_err());
        let ok = compress_chunk(b"hello").unwrap();
        assert_eq!(decompress_chunk(&ok).unwrap(), b"hello");
    }

    #[test]
    fn game_info_sanitizing_never_panics_on_multibyte_names() {
        let name = format!("a{}", "\u{e9}".repeat(200));
        let mut gi = GameInfo::default();
        gi.installed_packs.push(crate::state::PackInfo {
            id: crate::state::PackId { code: name.clone(), pack_type: crate::state::PackType::GamePack },
            name: format!("{name}\u{202e}"),
        });
        let out = sanitize_game_info(gi);
        let p = &out.installed_packs[0];
        assert!(p.name.chars().count() <= MAX_PACK_STRING_LEN && !p.name.contains('\u{202e}'));
        assert!(p.id.code.chars().count() <= MAX_PACK_STRING_LEN);
    }

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

    #[test]
    fn precheck_never_overwrites_a_different_local_file_on_plain_receive() {
        use ReplacePolicy::*;
        assert_eq!(precheck(None, "h", &MustNotExist), PreCheck::Download);
        assert_eq!(precheck(Some("h"), "h", &MustNotExist), PreCheck::AlreadyPresent);
        assert_eq!(precheck(Some("mine"), "h", &MustNotExist), PreCheck::Refuse(LOCAL_COPY_DIFFERS));
        assert_eq!(precheck(Some("mine"), "", &MustNotExist), PreCheck::Refuse(LOCAL_COPY_DIFFERS));
    }

    #[test]
    fn precheck_use_theirs_requires_unchanged_local_hash() {
        use ReplacePolicy::*;
        let planned = ReplaceIfHash("mine".to_string());
        assert_eq!(precheck(Some("mine"), "theirs", &planned), PreCheck::Download);
        assert_eq!(precheck(Some("edited"), "theirs", &planned), PreCheck::Refuse(LOCAL_CHANGED));
        assert_eq!(precheck(Some("theirs"), "theirs", &planned), PreCheck::AlreadyPresent);
        assert_eq!(precheck(None, "theirs", &planned), PreCheck::Download);
        assert_eq!(precheck(Some(""), "theirs", &ReplaceIfHash(String::new())), PreCheck::Refuse(LOCAL_CHANGED));
    }

    #[test]
    fn find_existing_matches_case_insensitively() {
        let dir = std::env::temp_dir().join(format!("synccrate-existing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Hair.package"), b"x").unwrap();
        let found = find_existing(&dir.join("hair.PACKAGE")).expect("case variant found");
        assert!(found.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("hair.package"));
        assert!(find_existing(&dir.join("other.package")).is_none());
        assert!(find_existing(&dir.join("missing").join("x.package")).is_none());

        let before = inspect_existing(&dir.join("Hair.package")).unwrap();
        assert!(destination_unchanged(&dir.join("Hair.package"), before.as_ref()));
        assert!(!destination_unchanged(&dir.join("hair.package"), None), "appeared file detected");
        std::fs::write(dir.join("Hair.package"), b"changed").unwrap();
        assert!(!destination_unchanged(&dir.join("Hair.package"), before.as_ref()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_rescans_empty_quick_or_stale_manifests() {
        use crate::state::{FileInfo, FileManifest};
        let mut m = FileManifest { files: Default::default(), generated_at: 1000 };
        assert!(host_manifest_needs_rescan(&m, 1000), "empty");
        m.files.insert("Mods/a.package".into(), FileInfo {
            relative_path: "Mods/a.package".into(),
            size: 1,
            hash: "h".into(),
            modified: 0,
            file_type: "Mod".into(),
        });
        assert!(!host_manifest_needs_rescan(&m, 1000));
        assert!(host_manifest_needs_rescan(&m, 1000 + HOST_MANIFEST_MAX_AGE_SECS + 1), "stale");
        m.files.get_mut("Mods/a.package").unwrap().hash.clear();
        assert!(host_manifest_needs_rescan(&m, 1000), "quick-scan hashes");
    }
}
