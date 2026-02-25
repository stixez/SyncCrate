use crate::network::transfer;
use crate::state::{AppState, FileInfo, Resolution, SyncAction, SyncPlan};
use crate::sync::diff;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn compute_sync_plan(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    peer_id: Option<String>,
) -> Result<SyncPlan, String> {
    let mut app_state = state.lock().await;

    let resolved_id = app_state.resolve_peer_id(peer_id)?;

    let conn = app_state
        .connections
        .get(&resolved_id)
        .ok_or("Peer not found")?;

    let remote = conn
        .remote_manifest
        .as_ref()
        .ok_or("No remote manifest available. Connect to a peer first.")?;

    let plan = diff::compute_diff(&app_state.local_manifest, remote);

    // Store plan on the peer connection
    let conn = app_state
        .connections
        .get_mut(&resolved_id)
        .ok_or("Peer disconnected")?;
    conn.sync_plan = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
pub async fn execute_sync(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    peer_id: Option<String>,
) -> Result<(), String> {
    let (plan, base_path, resolved_id) = {
        let mut app_state = state.lock().await;
        let resolved_id = app_state.resolve_peer_id(peer_id)?;
        let base = app_state.sims4_path.clone().ok_or("Sims 4 path not set")?;

        let conn = app_state
            .connections
            .get_mut(&resolved_id)
            .ok_or("Peer not found")?;

        if conn.is_syncing {
            return Err("Sync is already in progress".to_string());
        }
        let plan = conn.sync_plan.take().ok_or("No sync plan computed.")?;

        // Prevent sync while unresolved conflicts exist
        let has_conflicts = plan.actions.iter().any(|a| matches!(a, SyncAction::Conflict { .. }));
        if has_conflicts {
            conn.sync_plan = Some(plan);
            return Err("Resolve all conflicts before syncing".to_string());
        }

        conn.is_syncing = true;
        (plan, base, resolved_id)
    };

    // Run sync and ensure is_syncing is always reset
    let result = run_sync(&state, &app, &plan, &base_path, &resolved_id).await;

    {
        let mut app_state = state.lock().await;
        if let Some(conn) = app_state.connections.get_mut(&resolved_id) {
            conn.is_syncing = false;
        }
    }

    result
}

async fn run_sync(
    state: &tauri::State<'_, Arc<Mutex<AppState>>>,
    app: &tauri::AppHandle,
    plan: &SyncPlan,
    base_path: &str,
    peer_id: &str,
) -> Result<(), String> {
    let total_files = plan.actions.len() as u64;
    let mut files_done = 0u64;
    let mut bytes_done = 0u64;
    let mut sync_errors: Vec<String> = Vec::new();
    let state_arc = state.inner().clone();

    for action in &plan.actions {
        match action {
            SyncAction::ReceiveFromRemote(file_info) => {
                let _ = app.emit(
                    "sync-progress",
                    serde_json::json!({
                        "file": file_info.relative_path,
                        "bytes_sent": bytes_done,
                        "bytes_total": plan.total_bytes,
                        "files_done": files_done,
                        "files_total": total_files,
                        "peer_id": peer_id,
                    }),
                );

                match transfer::request_file(
                    &state_arc,
                    peer_id,
                    &file_info.relative_path,
                    base_path,
                )
                .await
                {
                    Ok(()) => {
                        files_done += 1;
                        bytes_done += file_info.size;
                    }
                    Err(e) => {
                        sync_errors.push(format!("{}: {}", file_info.relative_path, e));
                        let _ = app.emit(
                            "sync-error",
                            serde_json::json!({"message": format!("Failed to receive {}: {}", file_info.relative_path, e)}),
                        );
                    }
                }
            }
            SyncAction::SendToRemote(file_info) => {
                // The remote side will request files from us via the TCP handler.
                // Mark as done — the host serves files on demand.
                files_done += 1;
                bytes_done += file_info.size;
                let _ = app.emit(
                    "sync-progress",
                    serde_json::json!({
                        "file": file_info.relative_path,
                        "bytes_sent": bytes_done,
                        "bytes_total": plan.total_bytes,
                        "files_done": files_done,
                        "files_total": total_files,
                        "peer_id": peer_id,
                    }),
                );
            }
            SyncAction::Delete(path) => {
                match crate::utils::safe_join(base_path, path) {
                    Ok(full_path) => {
                        if let Err(e) = tokio::fs::remove_file(&full_path).await {
                            sync_errors.push(format!("Delete {}: {}", path, e));
                        }
                    }
                    Err(e) => {
                        sync_errors.push(format!("Delete {}: path rejected: {}", path, e));
                    }
                }
                files_done += 1;
            }
            SyncAction::Conflict { .. } => {
                // Skip conflicts — must be resolved individually first
            }
        }
    }

    let _ = app.emit(
        "sync-complete",
        serde_json::json!({
            "files_synced": files_done,
            "total_bytes": plan.total_bytes,
            "errors": sync_errors,
            "peer_id": peer_id,
        }),
    );

    if !sync_errors.is_empty() {
        return Err(format!("{} file(s) failed to sync", sync_errors.len()));
    }

    Ok(())
}

#[tauri::command]
pub async fn resolve_conflict(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    path: String,
    resolution: Resolution,
    peer_id: Option<String>,
) -> Result<SyncPlan, String> {
    let mut app_state = state.lock().await;

    let resolved_id = app_state.resolve_peer_id(peer_id)?;

    let conn = app_state
        .connections
        .get_mut(&resolved_id)
        .ok_or("Peer not found")?;

    if conn.is_syncing {
        return Err("Cannot resolve conflicts while sync is in progress".to_string());
    }

    // Clone remote file info before mutating sync_plan to satisfy borrow checker
    let remote_file = conn
        .remote_manifest
        .as_ref()
        .and_then(|m| m.files.get(&path))
        .cloned();

    if let Some(ref mut plan) = conn.sync_plan {
        plan.actions.retain(|action| {
            if let SyncAction::Conflict { local, .. } = action {
                return local.relative_path != path;
            }
            true
        });

        match resolution {
            Resolution::KeepMine => {}
            Resolution::UseTheirs => {
                if let Some(remote) = remote_file {
                    plan.actions.push(SyncAction::ReceiveFromRemote(remote));
                }
            }
            Resolution::KeepBoth => {
                if let Some(mut renamed) = remote_file {
                    let p = std::path::Path::new(&path);
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let stem = p.file_stem().and_then(|e| e.to_str()).unwrap_or("file");
                    let parent = p
                        .parent()
                        .map(|pp| pp.to_string_lossy().to_string())
                        .unwrap_or_default();

                    renamed.relative_path = if parent.is_empty() {
                        if ext.is_empty() {
                            format!("{}_remote", stem)
                        } else {
                            format!("{}_remote.{}", stem, ext)
                        }
                    } else if ext.is_empty() {
                        format!("{}/{}_remote", parent, stem)
                    } else {
                        format!("{}/{}_remote.{}", parent, stem, ext)
                    };

                    plan.actions.push(SyncAction::ReceiveFromRemote(renamed));
                }
            }
        }
    }

    conn.sync_plan
        .clone()
        .ok_or_else(|| "No sync plan available".to_string())
}

#[tauri::command]
pub async fn resolve_all_conflicts(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    strategy: String,
    peer_id: Option<String>,
) -> Result<SyncPlan, String> {
    if strategy != "use_newest" {
        return Err(format!("Unknown strategy: {}", strategy));
    }

    let mut app_state = state.lock().await;
    let resolved_id = app_state.resolve_peer_id(peer_id)?;

    let conn = app_state
        .connections
        .get_mut(&resolved_id)
        .ok_or("Peer not found")?;

    if conn.is_syncing {
        return Err("Cannot resolve conflicts while sync is in progress".to_string());
    }

    let remote_manifest = conn.remote_manifest.clone();

    if let Some(ref mut plan) = conn.sync_plan {
        // Collect conflicts and determine resolutions
        let mut to_receive: Vec<FileInfo> = Vec::new();
        plan.actions.retain(|action| {
            if let SyncAction::Conflict { local, remote } = action {
                if remote.modified > local.modified {
                    // Remote is newer — receive it
                    to_receive.push(remote.clone());
                }
                // else local is newer — keep mine (just remove the conflict)
                return false;
            }
            true
        });

        // Add receive actions for "use theirs" resolutions
        for file_info in to_receive {
            // Use remote manifest version if available (authoritative), fall back to conflict info
            let receive = remote_manifest
                .as_ref()
                .and_then(|m| m.files.get(&file_info.relative_path).cloned())
                .unwrap_or(file_info);
            plan.actions.push(SyncAction::ReceiveFromRemote(receive));
        }
    }

    conn.sync_plan
        .clone()
        .ok_or_else(|| "No sync plan available".to_string())
}
