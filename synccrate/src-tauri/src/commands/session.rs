use crate::network::discovery;
use crate::state::{AppState, PeerInfo, SessionInfo, SessionStatus, SessionType, SyncFolderPermissions};
use crate::network::protocol::{self, Message};
use rand::Rng;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

/// Sanitize a display name: strip control chars, limit length.
/// `connection-failed` event payload. A wrong-game rejection becomes a readable
/// message plus `host_game`, so the UI can offer "Switch to <game> and join".
fn connection_failed_payload(app_state: &AppState, error: &str) -> serde_json::Value {
    let Some(host_game) = protocol::parse_wrong_game(error) else {
        return serde_json::json!({ "message": error });
    };
    let label = |id: &str| {
        app_state
            .game_registry
            .games
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let host_label = label(host_game);
    serde_json::json!({
        "message": format!(
            "This host is sharing {}, but you have {} selected. Switch to {} and join again.",
            host_label,
            label(&app_state.active_game),
            host_label
        ),
        "host_game": host_game,
    })
}

fn sanitize_name(name: &str) -> Result<String, String> {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err("Name must be 1-64 characters".to_string());
    }
    Ok(trimmed.to_string())
}

pub(crate) fn client_attempt_is_active(state: &AppState, peer_id: &str) -> bool {
    state.session_type == SessionType::Client
        && state.pending_client_peer_id.as_deref() == Some(peer_id)
}

pub(crate) fn clear_failed_client_attempt_if_active(state: &mut AppState, peer_id: &str) -> bool {
    if !client_attempt_is_active(state, peer_id) {
        return false;
    }

    state.connections.remove(peer_id);
    state.session_type = SessionType::None;
    state.session_name.clear();
    state.local_display_name.clear();
    state.pending_client_peer_id = None;
    true
}

#[tauri::command]
pub async fn start_host(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    name: String,
    use_pin: Option<bool>,
    allowed_folders: Option<SyncFolderPermissions>,
) -> Result<SessionInfo, String> {
    let name = sanitize_name(&name)?;

    // Validate and read state, then drop lock before async bind
    let (port, mod_count, game_version, game_id) = {
        let app_state = state.lock().await;

        if app_state.session_type != SessionType::None {
            return Err("Already in a session. Disconnect first.".to_string());
        }

        if !app_state.game_paths.contains_key(&app_state.active_game) {
            let label = app_state.game_label(&app_state.active_game);
            return Err(format!("{} path not set. Please set it first.", label));
        }

        let gv = app_state
            .game_info
            .get(&app_state.active_game)
            .and_then(|gi| gi.game_version.clone());

        (app_state.session_port, app_state.local_manifest.files.len(), gv, app_state.active_game.clone())
    };

    // Bind TCP listener first — surfaces port conflicts to user before committing state
    let (listener, port) = crate::network::transfer::bind_listener(port).await?;

    // Optionally generate a 4-digit session PIN
    let pin = if use_pin.unwrap_or(false) {
        Some(format!("{:04}", rand::thread_rng().gen_range(1000..=9999)))
    } else {
        None
    };

    // Commit session state now that we know the port is available
    {
        let mut app_state = state.lock().await;
        app_state.session_type = SessionType::Host;
        app_state.session_port = port;
        app_state.session_name = name.clone();
        app_state.local_display_name = name.clone();
        app_state.session_pin = pin.clone();
        app_state.folder_permissions = allowed_folders.unwrap_or_default();
        app_state.chat.clear();
        app_state.chat.available = true;
    }

    // Warm the hash cache in the background so the first client's manifest request
    // (and the file transfers that follow) hit cached hashes instead of triggering a
    // slow full-folder SHA-256 pass during the handshake — which previously timed out
    // the joining peer on large folders (e.g. big Sims 4 Mods directories).
    {
        let warm_state = state.inner().clone();
        tokio::spawn(async move {
            match crate::commands::files::scan_files_inner(&warm_state, None, true).await {
                Ok(m) => log::info!("Host hash cache warmed ({} files)", m.files.len()),
                Err(e) => log::warn!("Background hash warm-up failed: {}", e),
            }
        });
    }

    // Start mDNS broadcast in background
    let app_handle = app.clone();
    let host_name = name.clone();
    let pin_required = pin.is_some();
    let node_id = state.lock().await.local_node_id.clone().unwrap_or_default();
    tokio::spawn(async move {
        if let Err(e) = discovery::start_broadcast(host_name, port, mod_count, pin_required, game_version, game_id, node_id).await {
            log::error!("Discovery broadcast error: {}", e);
            let _ = app_handle.emit("discovery-unavailable", serde_json::json!({"message": e}));
        }
    });

    // Bring up internet connectivity (iroh) so friends outside the LAN can join
    // with the join code. Failure only disables internet joins.
    {
        let net_state = state.inner().clone();
        let net_app = app.clone();
        let net_events = crate::event_sink::from_app(&net_app);
        tokio::spawn(async move {
            match crate::network::iroh_net::endpoint(&net_state, &net_events).await {
                Ok(ep) => {
                    ep.online().await;
                    log::info!("Internet joining ready ({})", ep.id().fmt_short());
                }
                Err(e) => {
                    log::warn!("{}", e);
                    let _ = net_app.emit("internet-unavailable", serde_json::json!({"message": e}));
                }
            }
        });
    }

    // Run TCP accept loop in background (already bound)
    let app_handle = app.clone();
    let state_clone = state.inner().clone();
    tokio::spawn(async move {
        crate::network::transfer::run_listener(listener, state_clone, crate::event_sink::from_app(&app_handle)).await;
    });

    Ok(SessionInfo {
        session_type: SessionType::Host,
        name,
        port,
        peer_count: 0,
    })
}

#[tauri::command]
pub async fn start_join(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    name: String,
) -> Result<Vec<PeerInfo>, String> {
    let name = sanitize_name(&name)?;
    let app_state = state.lock().await;

    if app_state.session_type != SessionType::None {
        return Err("Already in a session. Disconnect first.".to_string());
    }

    drop(app_state);

    let _ = app;
    let peers = discovery::scan_for_hosts().await?;

    let mut app_state = state.lock().await;
    app_state.discovered_peers = peers.clone();
    app_state.local_display_name = name;

    Ok(peers)
}

#[tauri::command]
pub async fn connect_to_peer(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    peer_id: String,
    pin: Option<String>,
) -> Result<SessionInfo, String> {
    let mut app_state = state.lock().await;

    if app_state.session_type != SessionType::None {
        return Err("Already in a session. Disconnect first.".to_string());
    }

    let peer = app_state
        .discovered_peers
        .iter()
        .find(|p| p.id == peer_id)
        .cloned()
        .ok_or("Peer not found")?;

    app_state.session_type = SessionType::Client;
    app_state.session_name = peer.name.clone();

    let connection_peer_id = peer.id.clone();
    app_state.pending_client_peer_id = Some(connection_peer_id.clone());
    let state_clone = state.inner().clone();
    drop(app_state);

    // Connect to host in background
    let app_handle = app.clone();
    let connect_pin = pin;
    tokio::spawn(async move {
        let addresses = if peer.addresses.is_empty() {
            vec![peer.ip.clone()]
        } else {
            peer.addresses.clone()
        };
        if let Err(e) = crate::network::transfer::connect_to_host(
            &addresses,
            peer.port,
            None,
            &connection_peer_id,
            state_clone.clone(),
            crate::event_sink::from_app(&app_handle),
            connect_pin,
        ).await {
            log::error!("Connection error: {}", e);
            let mut app_state = state_clone.lock().await;
            if clear_failed_client_attempt_if_active(&mut app_state, &connection_peer_id) {
                let _ = app_handle.emit("connection-failed", connection_failed_payload(&app_state, &e));
            }
        }
    });

    Ok(SessionInfo {
        session_type: SessionType::Client,
        name: peer.name,
        port: peer.port,
        peer_count: 1,
    })
}

#[tauri::command]
pub async fn disconnect(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    disconnect_inner(state.inner(), &app).await;
    Ok(())
}

/// Leave the current session (stop hosting / disconnect from the host). Shared by
/// the `disconnect` command and the tray menu; emits `peer-disconnected` with
/// name "all" so the frontend resets its session view.
pub async fn disconnect_inner(state: &Arc<Mutex<AppState>>, app: &tauri::AppHandle) {
    let streams: Vec<_> = {
        let app_state = state.lock().await;
        app_state.connections.values().map(|c| c.stream.clone()).collect()
    };

    for stream in streams {
        let mut s = stream.lock().await;
        let _ = protocol::send_message(&mut *s, &Message::Disconnect).await;
    }

    crate::network::transfer::reset_cancellation_token().await;

    let mut app_state = state.lock().await;
    app_state.connections.clear();
    app_state.session_type = SessionType::None;
    app_state.session_name.clear();
    app_state.local_display_name.clear();
    app_state.pending_client_peer_id = None;
    app_state.session_pin = None;
    app_state.folder_permissions = SyncFolderPermissions::default();
    app_state.discovered_peers.clear();

    discovery::stop_broadcast().await;

    let _ = app.emit("peer-disconnected", serde_json::json!({"name": "all", "clean": true}));
}

#[tauri::command]
pub async fn disconnect_peer(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    peer_id: String,
) -> Result<(), String> {
    let conn = {
        let mut app_state = state.lock().await;
        app_state
            .connections
            .remove(&peer_id)
            .ok_or_else(|| format!("Peer '{}' not found", peer_id))?
    };

    {
        let mut s = conn.stream.lock().await;
        let _ = protocol::send_message(&mut *s, &Message::Disconnect).await;
    }

    let _ = app.emit(
        "peer-disconnected",
        serde_json::json!({"name": conn.info.name, "peer_id": peer_id}),
    );

    Ok(())
}

#[tauri::command]
pub async fn get_session_status(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<SessionStatus, String> {
    let app_state = state.lock().await;
    let host_ips = if app_state.session_type == SessionType::Host {
        crate::network::netutil::host_display_ips()
    } else {
        vec![]
    };
    Ok(SessionStatus {
        session_type: app_state.session_type.clone(),
        name: app_state.session_name.clone(),
        port: app_state.session_port,
        peers: app_state.peers(),
        is_syncing: app_state.is_any_syncing(),
        pin: app_state.session_pin.clone(),
        host_ips,
        discovery_active: app_state.session_type == SessionType::Host && discovery::discovery_active(),
    })
}

#[tauri::command]
pub async fn connect_by_ip(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    ip: String,
    port: u16,
    name: String,
    pin: Option<String>,
) -> Result<SessionInfo, String> {
    let name = sanitize_name(&name)?;
    ip.parse::<std::net::IpAddr>().map_err(|_| "Invalid IP address".to_string())?;
    start_direct_connection(state.inner(), app, vec![ip.clone()], port, None, name, pin, ip).await
}

/// Join using a host's join code (addresses + port + optional PIN in one string).
#[tauri::command]
pub async fn connect_by_code(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    code: String,
    name: String,
    pin: Option<String>,
) -> Result<SessionInfo, String> {
    let name = sanitize_name(&name)?;
    let info = crate::network::joincode::decode(&code)?;
    let ranked: Vec<String> = crate::network::netutil::rank_addresses(
        &info.addresses.iter().map(|a| std::net::IpAddr::V4(*a)).collect::<Vec<_>>(),
    )
    .iter()
    .map(|a| a.to_string())
    .collect();
    let label = ranked.first().cloned().unwrap_or_default();
    // A PIN typed by the user wins over the one embedded in the code (e.g. the
    // host restarted hosting and got a new PIN but the code was shared earlier).
    let pin = pin.filter(|p| !p.trim().is_empty()).or(info.pin);
    let internet_id = match info.internet_id {
        Some(bytes) => Some(
            iroh::EndpointId::from_bytes(&bytes)
                .map_err(|_| "That join code isn't valid — check it was copied completely.".to_string())?,
        ),
        None => None,
    };
    let label = if label.is_empty() { "Internet".to_string() } else { label };
    start_direct_connection(state.inner(), app, ranked, info.port, internet_id, name, pin, label).await
}

/// The host's join code for the current session.
#[tauri::command]
pub async fn get_join_code(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<String, String> {
    join_code_for(state.inner()).await
}

/// Shared with `commands::modpack::create_pack_inner`, which embeds the
/// current join code in an exported pack while hosting.
pub(crate) async fn join_code_for(state: &Arc<Mutex<AppState>>) -> Result<String, String> {
    let (port, pin) = {
        let app_state = state.lock().await;
        if app_state.session_type != SessionType::Host {
            return Err("Join codes are only available while hosting".to_string());
        }
        (app_state.session_port, app_state.session_pin.clone())
    };
    let addresses: Vec<std::net::Ipv4Addr> = tokio::task::spawn_blocking(crate::network::netutil::host_display_ips)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|ip| ip.parse().ok())
        // Only the best LAN address: the internet id already reaches the host
        // on the same network too (iroh connects directly), and each extra
        // address makes the code longer.
        .take(1)
        .collect();
    let internet_id = Some(*crate::network::iroh_net::local_id().as_bytes());
    crate::network::joincode::encode(&crate::network::joincode::JoinInfo { addresses, port, pin, internet_id })
}

/// Shared by Connect-by-IP and join codes: mark the pending client session and
/// connect in the background (the frontend waits for `peer-connected` /
/// `connection-failed`).
pub(crate) async fn start_direct_connection(
    state: &Arc<Mutex<AppState>>,
    app: tauri::AppHandle,
    addresses: Vec<String>,
    port: u16,
    internet_id: Option<iroh::EndpointId>,
    name: String,
    pin: Option<String>,
    label: String,
) -> Result<SessionInfo, String> {
    if port < 1024 {
        return Err("Port must be 1024 or higher".to_string());
    }

    let mut app_state = state.lock().await;
    if app_state.session_type != SessionType::None {
        return Err("Already in a session. Disconnect first.".to_string());
    }

    app_state.session_type = SessionType::Client;
    app_state.session_name = format!("{}:{}", label, port);
    app_state.local_display_name = name;

    let peer_id = uuid::Uuid::new_v4().to_string();
    app_state.pending_client_peer_id = Some(peer_id.clone());
    let state_clone = state.clone();
    drop(app_state);

    let app_handle = app.clone();
    let connect_peer_id = peer_id.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::network::transfer::connect_to_host(
            &addresses, port, internet_id, &connect_peer_id, state_clone.clone(), crate::event_sink::from_app(&app_handle), pin,
        ).await {
            log::error!("Direct connection error: {}", e);
            let mut app_state = state_clone.lock().await;
            if clear_failed_client_attempt_if_active(&mut app_state, &connect_peer_id) {
                let _ = app_handle.emit("connection-failed", connection_failed_payload(&app_state, &e));
            }
        }
    });

    Ok(SessionInfo {
        session_type: SessionType::Client,
        name: label,
        port,
        peer_count: 1,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct HostUpdates {
    pub files: usize,
    pub bytes: u64,
}

/// Files the host has that we don't — exactly the `ReceiveFromRemote` actions
/// `compute_sync_plan` would produce (a missing path; changed files become
/// conflicts), minus disallowed content types, paths outside this game's
/// content folders and exclude patterns. Paths are matched like the diff does
/// (`sync::diff::match_key`: case, `.disabled`, `_Disabled/`). Only paths
/// matter, so a quick-scan local manifest (empty hashes) needs no re-hash.
pub(crate) fn count_new_host_files(
    local: &crate::state::FileManifest,
    remote: &crate::state::FileManifest,
    content_types: &[crate::registry::ContentType],
    allowed: impl Fn(&crate::state::FileInfo) -> bool,
    exclude_patterns: &[String],
) -> HostUpdates {
    use crate::sync::diff::{match_key, path_accepted_by};
    let local_keys: std::collections::HashSet<String> =
        local.files.keys().map(|k| match_key(k)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = HostUpdates::default();
    for (path, info) in &remote.files {
        let key = match_key(path);
        if local_keys.contains(&key)
            || !path_accepted_by(content_types, path)
            || !allowed(info)
            || !seen.insert(key)
        {
            continue;
        }
        if exclude_patterns.iter().any(|p| crate::commands::sync::glob_matches(p, path)) {
            continue;
        }
        out.files += 1;
        out.bytes += info.size;
    }
    out
}

/// Client only: re-fetch the host's manifest over the live connection and count
/// the files we would download. Returns zero while not connected or syncing.
#[tauri::command]
pub async fn check_host_updates(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<HostUpdates, String> {
    let peer_id = {
        let app_state = state.lock().await;
        if app_state.session_type != SessionType::Client || app_state.is_any_syncing() {
            return Ok(HostUpdates::default());
        }
        match app_state.connections.keys().next() {
            Some(id) => id.clone(),
            None => return Ok(HostUpdates::default()),
        }
    };

    let remote = crate::network::transfer::refresh_remote_manifest(state.inner(), &peer_id).await?;
    let patterns = crate::commands::sync::read_exclude_patterns();
    let app_state = state.lock().await;
    let content_types = crate::commands::files::get_game_def(&app_state.game_registry, &app_state.active_game)
        .map(|g| g.content_types.clone())
        .unwrap_or_default();
    Ok(count_new_host_files(
        &app_state.local_manifest,
        &remote,
        &content_types,
        |f| app_state.is_file_info_allowed(f),
        &patterns,
    ))
}

#[tauri::command]
pub async fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub async fn check_port_available(port: u16) -> Result<bool, String> {
    if port < 1024 {
        return Err("Port must be 1024 or higher".to_string());
    }
    match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[tauri::command]
pub async fn set_session_port(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    port: u16,
) -> Result<(), String> {
    if port < 1024 {
        return Err("Port must be 1024 or higher".to_string());
    }
    let mut app_state = state.lock().await;
    if app_state.session_type != SessionType::None {
        return Err("Cannot change port while in a session".to_string());
    }
    app_state.session_port = port;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(paths: &[(&str, u64)]) -> crate::state::FileManifest {
        let mut m = crate::state::FileManifest::default();
        for (p, size) in paths {
            m.files.insert(p.to_string(), crate::state::FileInfo {
                relative_path: p.to_string(),
                size: *size,
                hash: String::new(),
                modified: 0,
                file_type: "Mod".to_string(),
            });
        }
        m
    }

    #[test]
    fn count_new_host_files_counts_only_missing_allowed_unexcluded() {
        let local = manifest(&[("Mods/a.package", 10)]);
        let remote = manifest(&[
            ("Mods/a.package", 99),     // present locally (changed = conflict, not counted)
            ("Mods/b.package", 20),     // new
            ("Mods/c.tmp", 5),          // excluded by pattern
            ("Saves/s.save", 7),        // disallowed
        ]);
        let out = count_new_host_files(
            &local,
            &remote,
            &content_types(),
            |f| !f.relative_path.starts_with("Saves/"),
            &["*.tmp".to_string()],
        );
        assert_eq!(out, HostUpdates { files: 1, bytes: 20 });
    }

    fn content_types() -> Vec<crate::registry::ContentType> {
        let def = crate::registry::load_registry()
            .games
            .into_iter()
            .find(|g| g.id == "sims4")
            .expect("sims4 in registry");
        let mut cts = def.content_types;
        // Let the test's `.tmp` file through so the exclude pattern is what drops it.
        for ct in &mut cts {
            ct.extensions.push("tmp".to_string());
        }
        cts
    }

    #[test]
    fn count_new_host_files_matches_like_the_diff_and_skips_foreign() {
        let local = manifest(&[("Mods/cc/hair.package.disabled", 10)]);
        let remote = manifest(&[
            ("Mods/CC/Hair.package", 10), // disabled/case twin of a local file
            ("mod/truck.scs", 30),        // another game's folder
            ("Mods/new.package", 5),
        ]);
        let out = count_new_host_files(&local, &remote, &content_types(), |_| true, &[]);
        assert_eq!(out, HostUpdates { files: 1, bytes: 5 });
    }

    #[test]
    fn stale_failed_attempt_does_not_clear_replaced_client_session() {
        let mut state = AppState::default();
        state.session_type = SessionType::Client;
        state.session_name = "Host B".to_string();
        state.local_display_name = "Alice".to_string();
        state.pending_client_peer_id = Some("new-attempt".to_string());

        clear_failed_client_attempt_if_active(&mut state, "old-attempt");

        assert_eq!(state.session_type, SessionType::Client);
        assert_eq!(state.session_name, "Host B");
        assert_eq!(state.local_display_name, "Alice");
        assert_eq!(state.pending_client_peer_id.as_deref(), Some("new-attempt"));
    }

    #[test]
    fn active_failed_attempt_clears_client_session() {
        let mut state = AppState::default();
        state.session_type = SessionType::Client;
        state.session_name = "Host A".to_string();
        state.local_display_name = "Alice".to_string();
        state.pending_client_peer_id = Some("active-attempt".to_string());

        clear_failed_client_attempt_if_active(&mut state, "active-attempt");

        assert_eq!(state.session_type, SessionType::None);
        assert!(state.session_name.is_empty());
        assert!(state.local_display_name.is_empty());
        assert!(state.pending_client_peer_id.is_none());
    }
}
