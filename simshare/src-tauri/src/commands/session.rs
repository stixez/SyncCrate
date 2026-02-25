use crate::network::discovery;
use crate::state::{AppState, PeerInfo, SessionInfo, SessionStatus, SessionType};
use crate::network::protocol::{self, Message};
use rand::Rng;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

/// Sanitize a display name: strip control chars, limit length.
fn sanitize_name(name: &str) -> Result<String, String> {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err("Name must be 1-64 characters".to_string());
    }
    Ok(trimmed.to_string())
}

#[tauri::command]
pub async fn start_host(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    name: String,
    use_pin: Option<bool>,
) -> Result<SessionInfo, String> {
    let name = sanitize_name(&name)?;

    // Validate and read state, then drop lock before async bind
    let (port, mod_count) = {
        let app_state = state.lock().await;

        if app_state.session_type != SessionType::None {
            return Err("Already in a session. Disconnect first.".to_string());
        }

        if app_state.sims4_path.is_none() {
            return Err("Sims 4 path not set. Please set it first.".to_string());
        }

        (app_state.session_port, app_state.local_manifest.files.len())
    };

    // Bind TCP listener first — surfaces port conflicts to user before committing state
    let listener = crate::network::transfer::bind_listener(port).await?;

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
        app_state.session_name = name.clone();
        app_state.local_display_name = name.clone();
        app_state.session_pin = pin.clone();
    }

    // Start mDNS broadcast in background
    let app_handle = app.clone();
    let host_name = name.clone();
    let pin_required = pin.is_some();
    tokio::spawn(async move {
        if let Err(e) = discovery::start_broadcast(host_name, port, mod_count, pin_required, app_handle).await {
            log::error!("mDNS broadcast error: {}", e);
        }
    });

    // Run TCP accept loop in background (already bound)
    let app_handle = app.clone();
    let state_clone = state.inner().clone();
    tokio::spawn(async move {
        crate::network::transfer::run_listener(listener, state_clone, app_handle).await;
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

    let peers = discovery::scan_for_hosts(app).await.map_err(|e| e.to_string())?;

    let mut app_state = state.lock().await;
    app_state.discovered_peers = peers.clone();
    // Store the user's chosen display name for use during connect
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

    let peer = app_state
        .discovered_peers
        .iter()
        .find(|p| p.id == peer_id)
        .cloned()
        .ok_or("Peer not found")?;

    app_state.session_type = SessionType::Client;
    app_state.session_name = peer.name.clone();

    let connection_peer_id = peer.id.clone();
    let state_clone = state.inner().clone();
    drop(app_state);

    // Connect to host in background
    let app_handle = app.clone();
    let connect_pin = pin;
    tokio::spawn(async move {
        if let Err(e) = crate::network::transfer::connect_to_host(
            &peer.ip,
            peer.port,
            &connection_peer_id,
            state_clone.clone(),
            app_handle.clone(),
            connect_pin,
        ).await {
            log::error!("Connection error: {}", e);
            // Reset session state on failure
            let mut app_state = state_clone.lock().await;
            app_state.session_type = SessionType::None;
            app_state.session_name.clear();
            app_state.connections.clear();
            let _ = app_handle.emit(
                "connection-failed",
                serde_json::json!({"message": format!("{}", e)}),
            );
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
    // Collect streams, then drop state lock before doing network I/O
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
    app_state.session_pin = None;
    app_state.discovered_peers.clear();

    discovery::stop_broadcast().await;

    let _ = app.emit("peer-disconnected", serde_json::json!({"name": "all"}));

    Ok(())
}

#[tauri::command]
pub async fn disconnect_peer(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    peer_id: String,
) -> Result<(), String> {
    // Remove from map and drop state lock before network I/O
    let conn = {
        let mut app_state = state.lock().await;
        app_state
            .connections
            .remove(&peer_id)
            .ok_or_else(|| format!("Peer '{}' not found", peer_id))?
    };

    // Send Disconnect without holding the state lock
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
    Ok(SessionStatus {
        session_type: app_state.session_type.clone(),
        name: app_state.session_name.clone(),
        port: app_state.session_port,
        peers: app_state.peers(),
        is_syncing: app_state.is_any_syncing(),
        pin: app_state.session_pin.clone(),
    })
}

#[tauri::command]
pub async fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
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
