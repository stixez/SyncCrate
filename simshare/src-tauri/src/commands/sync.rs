use crate::network::transfer;
use crate::state::{AppState, FileInfo, Resolution, SyncAction, SyncPlan};
use crate::sync::diff;
use crate::utils;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn compute_sync_plan(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    peer_id: Option<String>,
) -> Result<SyncPlan, String> {
    let resolved_id = {
        let app_state = state.lock().await;
        app_state.resolve_peer_id(peer_id)?
    };

    // If we don't have the peer's manifest yet (race between connection
    // and the user clicking "Compare & Sync"), wait briefly for it to
    // arrive via the normal message loop rather than erroring immediately.
    {
        let needs_wait = {
            let app_state = state.lock().await;
            let conn = app_state
                .connections
                .get(&resolved_id)
                .ok_or("Peer not found")?;
            conn.remote_manifest.is_none()
        };
        if needs_wait {
            for _ in 0..30 {
                // Drop state lock while sleeping so the message loop can store the manifest
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let app_state = state.lock().await;
                match app_state.connections.get(&resolved_id) {
                    Some(conn) if conn.remote_manifest.is_some() => break,
                    Some(_) => {}
                    None => return Err("Peer disconnected".to_string()),
                }
            }
        }
    }

    let mut app_state = state.lock().await;

    let conn = app_state
        .connections
        .get(&resolved_id)
        .ok_or("Peer not found")?;

    let remote = conn
        .remote_manifest
        .as_ref()
        .ok_or("Peer's mod list hasn't arrived yet. Please try again in a moment.")?;

    let mut plan = diff::compute_diff(&app_state.local_manifest, remote);

    // Filter out actions for folders disabled by host permissions
    let perms = &app_state.folder_permissions;
    plan.actions.retain(|action| {
        let file_type = match action {
            SyncAction::SendToRemote(f) => Some(&f.file_type),
            SyncAction::ReceiveFromRemote(f) => Some(&f.file_type),
            SyncAction::Conflict { local, .. } => Some(&local.file_type),
            SyncAction::Delete(_) => None,
        };
        file_type.map_or(true, |ft| perms.is_file_allowed(ft))
    });

    // Recalculate total_bytes after filtering
    plan.total_bytes = plan.actions.iter().map(|action| match action {
        SyncAction::SendToRemote(f) => f.size,
        SyncAction::ReceiveFromRemote(f) => f.size,
        SyncAction::Conflict { local, remote } => local.size.max(remote.size),
        SyncAction::Delete(_) => 0,
    }).sum();

    // Apply stored exclude patterns to pre-populate excluded list
    let patterns = read_exclude_patterns();
    if !patterns.is_empty() {
        let mut excluded = Vec::new();
        for action in &plan.actions {
            let path = match action {
                SyncAction::SendToRemote(f) => &f.relative_path,
                SyncAction::ReceiveFromRemote(f) => &f.relative_path,
                SyncAction::Conflict { local, .. } => &local.relative_path,
                SyncAction::Delete(p) => p,
            };
            if patterns.iter().any(|pat| glob_matches(pat, path)) {
                excluded.push(path.clone());
            }
        }
        plan.excluded = excluded;

        // Recalculate total_bytes to exclude excluded files
        plan.total_bytes = plan.actions.iter()
            .filter(|action| {
                let path = match action {
                    SyncAction::SendToRemote(f) => &f.relative_path,
                    SyncAction::ReceiveFromRemote(f) => &f.relative_path,
                    SyncAction::Conflict { local, .. } => &local.relative_path,
                    SyncAction::Delete(p) => p,
                };
                !plan.excluded.contains(path)
            })
            .map(|action| match action {
                SyncAction::SendToRemote(f) => f.size,
                SyncAction::ReceiveFromRemote(f) => f.size,
                SyncAction::Conflict { local, remote } => local.size.max(remote.size),
                SyncAction::Delete(_) => 0,
            })
            .sum();
    }

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
        let base = app_state.active_game_path()?;

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
    // Count only non-excluded actions for accurate progress
    let total_files = plan.actions.iter()
        .filter(|action| {
            let path = match action {
                SyncAction::SendToRemote(f) => Some(&f.relative_path),
                SyncAction::ReceiveFromRemote(f) => Some(&f.relative_path),
                SyncAction::Conflict { local, .. } => Some(&local.relative_path),
                SyncAction::Delete(p) => Some(p),
            };
            path.map_or(true, |p| !plan.excluded.contains(p))
        })
        .count() as u64;
    let mut files_done = 0u64;
    let mut bytes_done = 0u64;
    let mut sync_errors: Vec<String> = Vec::new();
    let state_arc = state.inner().clone();

    for action in &plan.actions {
        // Skip excluded actions
        let action_path = match action {
            SyncAction::SendToRemote(f) => Some(&f.relative_path),
            SyncAction::ReceiveFromRemote(f) => Some(&f.relative_path),
            SyncAction::Conflict { local, .. } => Some(&local.relative_path),
            SyncAction::Delete(p) => Some(p),
        };
        if let Some(path) = action_path {
            if plan.excluded.contains(path) {
                continue;
            }
        }

        match action {
            SyncAction::ReceiveFromRemote(file_info) => {
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
                        files_done += 1;
                        sync_errors.push(format!("{}: {}", file_info.relative_path, e));
                        let _ = app.emit(
                            "sync-error",
                            serde_json::json!({"message": format!("Failed to receive {}: {}", file_info.relative_path, e)}),
                        );
                    }
                }
                // Emit progress AFTER transfer so the last file reaches 100%
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

// --- Selective Sync helpers ---

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");

    if pattern == "*" {
        return true;
    }

    // Extension pattern like "*.ts4script"
    if let Some(ext) = pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{}", ext));
    }

    // Directory prefix pattern like "Mods/subfolder/*"
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return path.starts_with(&format!("{}/", prefix));
    }

    // Exact match
    pattern == path
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SyncConfig {
    exclude_patterns: Vec<String>,
}

fn read_exclude_patterns() -> Vec<String> {
    let path = utils::sync_config_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<SyncConfig>(&data) {
                return config.exclude_patterns;
            }
        }
    }
    Vec::new()
}

#[tauri::command]
pub async fn update_sync_selection(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    peer_id: String,
    excluded_paths: Vec<String>,
) -> Result<SyncPlan, String> {
    let mut app_state = state.lock().await;

    let conn = app_state
        .connections
        .get_mut(&peer_id)
        .ok_or("Peer not found")?;

    if conn.is_syncing {
        return Err("Cannot modify selection while sync is in progress".to_string());
    }

    if let Some(ref mut plan) = conn.sync_plan {
        plan.excluded = excluded_paths;

        // Recalculate total_bytes excluding excluded items
        let excluded = plan.excluded.clone();
        let total: u64 = plan
            .actions
            .iter()
            .filter(|action| {
                let path = match action {
                    SyncAction::SendToRemote(f) => &f.relative_path,
                    SyncAction::ReceiveFromRemote(f) => &f.relative_path,
                    SyncAction::Conflict { local, .. } => &local.relative_path,
                    SyncAction::Delete(_) => return true,
                };
                !excluded.contains(path)
            })
            .map(|action| match action {
                SyncAction::SendToRemote(f) => f.size,
                SyncAction::ReceiveFromRemote(f) => f.size,
                SyncAction::Conflict { local, remote } => local.size.max(remote.size),
                SyncAction::Delete(_) => 0,
            })
            .sum();
        plan.total_bytes = total;
    }

    conn.sync_plan
        .clone()
        .ok_or_else(|| "No sync plan available".to_string())
}

#[tauri::command]
pub async fn set_exclude_patterns(patterns: Vec<String>) -> Result<(), String> {
    if patterns.len() > 100 {
        return Err("Too many exclude patterns (max 100)".to_string());
    }
    for pat in &patterns {
        if pat.len() > 256 {
            return Err("Pattern too long (max 256 characters)".to_string());
        }
        if pat.contains("..") || pat.contains('\0') {
            return Err("Invalid pattern: contains path traversal or null bytes".to_string());
        }
    }

    let config = SyncConfig {
        exclude_patterns: patterns,
    };
    let path = utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_exclude_patterns() -> Result<Vec<String>, String> {
    Ok(read_exclude_patterns())
}
