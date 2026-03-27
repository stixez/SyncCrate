use crate::network::transfer;
use crate::state::{is_file_allowed, file_type_to_content_id, AppState, FileInfo, Resolution, SyncAction, SyncPlan};
use crate::sync::diff;
use crate::utils;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SyncCheckpoint {
    game: String,
    peer_id: String,
    plan_hash: String,
    completed_files: Vec<String>,
    total_files: u64,
    total_bytes: u64,
    started_at: u64,
}

fn checkpoint_path() -> std::path::PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    config.join("synccrate").join("sync_progress.json")
}

fn read_checkpoint() -> Option<SyncCheckpoint> {
    let path = checkpoint_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            return serde_json::from_str(&data).ok();
        }
    }
    None
}

fn write_checkpoint(checkpoint: &SyncCheckpoint) {
    let path = checkpoint_path();
    if let Ok(data) = serde_json::to_string_pretty(checkpoint) {
        let _ = std::fs::write(&path, data);
    }
}

fn delete_checkpoint() {
    let path = checkpoint_path();
    let _ = std::fs::remove_file(&path);
}

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

    let mut plan = diff::compute_diff(&app_state.local_manifest, remote);

    // Filter out actions for content types disabled by host permissions
    let perms = &app_state.folder_permissions;
    let game_def = app_state.game_registry.games.iter()
        .find(|g| g.id == app_state.active_game);
    let content_types: Vec<crate::registry::ContentType> = game_def
        .map(|def| def.content_types.clone())
        .unwrap_or_default();

    plan.actions.retain(|action| {
        let file_type = match action {
            SyncAction::SendToRemote(f) => Some(&f.file_type),
            SyncAction::ReceiveFromRemote(f) => Some(&f.file_type),
            SyncAction::Conflict { local, .. } => Some(&local.file_type),
            SyncAction::Delete(_) => None,
        };
        file_type.map_or(true, |ft| {
            // Map file_type to content type ID and check permissions
            let content_id = file_type_to_content_id(ft, &content_types);
            match content_id {
                Some(id) => is_file_allowed(perms, &id),
                None => true, // Unknown file types are allowed by default
            }
        })
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

    // Check for resumable checkpoint
    let mut resumed_files: u64 = 0;
    if let Some(checkpoint) = read_checkpoint() {
        let plan_hash = diff::compute_plan_hash(&plan);
        if checkpoint.game == app_state.active_game
            && checkpoint.peer_id == resolved_id
            && checkpoint.plan_hash == plan_hash
            && !checkpoint.completed_files.is_empty()
        {
            let completed_set: std::collections::HashSet<&str> = checkpoint
                .completed_files
                .iter()
                .map(|s| s.as_str())
                .collect();

            plan.actions.retain(|action| {
                let path = match action {
                    SyncAction::ReceiveFromRemote(f) => &f.relative_path,
                    SyncAction::Delete(p) => p,
                    _ => return true,
                };
                !completed_set.contains(path.as_str())
            });

            resumed_files = checkpoint.completed_files.len() as u64;

            plan.total_bytes = plan.actions.iter().map(|action| match action {
                SyncAction::SendToRemote(f) => f.size,
                SyncAction::ReceiveFromRemote(f) => f.size,
                SyncAction::Conflict { local, remote } => local.size.max(remote.size),
                SyncAction::Delete(_) => 0,
            }).sum();

            log::info!("Resuming sync: {} files already completed", resumed_files);
        } else {
            delete_checkpoint();
        }
    }

    plan.resumed_files = resumed_files;

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

        let has_conflicts = plan.actions.iter().any(|a| matches!(a, SyncAction::Conflict { .. }));
        if has_conflicts {
            conn.sync_plan = Some(plan);
            return Err("Resolve all conflicts before syncing".to_string());
        }

        conn.is_syncing = true;
        (plan, base, resolved_id)
    };

    // Auto-backup before sync if enabled
    let config = read_sync_config();
    if config.auto_backup_before_sync {
        log::info!("Creating pre-sync auto-backup");
        if let Err(e) = crate::commands::backup::create_auto_backup(
            state.inner(),
            &app,
            "Pre-sync",
        ).await {
            log::warn!("Pre-sync auto-backup failed: {}", e);
            // Don't block sync on backup failure
        }
    }

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

    let plan_hash = diff::compute_plan_hash(plan);
    let game_id = {
        let app_state = state.lock().await;
        app_state.active_game.clone()
    };
    let mut checkpoint = SyncCheckpoint {
        game: game_id,
        peer_id: peer_id.to_string(),
        plan_hash,
        completed_files: Vec::new(),
        total_files,
        total_bytes: plan.total_bytes,
        started_at: crate::utils::timestamp_now(),
    };
    write_checkpoint(&checkpoint);

    for action in &plan.actions {
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
                        checkpoint.completed_files.push(file_info.relative_path.clone());
                        write_checkpoint(&checkpoint);
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
                        } else {
                            checkpoint.completed_files.push(path.clone());
                            write_checkpoint(&checkpoint);
                        }
                    }
                    Err(e) => {
                        sync_errors.push(format!("Delete {}: path rejected: {}", path, e));
                    }
                }
                files_done += 1;
            }
            SyncAction::Conflict { .. } => {}
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

    delete_checkpoint();

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
        let mut to_receive: Vec<FileInfo> = Vec::new();
        plan.actions.retain(|action| {
            if let SyncAction::Conflict { local, remote } = action {
                if remote.modified > local.modified {
                    to_receive.push(remote.clone());
                }
                return false;
            }
            true
        });

        for file_info in to_receive {
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

    if let Some(ext) = pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{}", ext));
    }

    if let Some(prefix) = pattern.strip_suffix("/*") {
        return path.starts_with(&format!("{}/", prefix));
    }

    pattern == path
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct SyncConfig {
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub auto_backup_before_sync: bool,
    #[serde(default)]
    pub auto_backup_scheduled: bool,
    #[serde(default = "default_backup_interval")]
    pub auto_backup_interval_hours: u32,
    #[serde(default = "default_backup_max_count")]
    pub auto_backup_max_count: u32,
}

fn default_backup_interval() -> u32 { 4 }
fn default_backup_max_count() -> u32 { 5 }

pub fn read_sync_config() -> SyncConfig {
    let path = utils::sync_config_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<SyncConfig>(&data) {
                return config;
            }
        }
    }
    SyncConfig::default()
}

fn read_exclude_patterns() -> Vec<String> {
    read_sync_config().exclude_patterns
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

    let mut config = read_sync_config();
    config.exclude_patterns = patterns;
    let path = utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_exclude_patterns() -> Result<Vec<String>, String> {
    Ok(read_exclude_patterns())
}

#[tauri::command]
pub async fn get_auto_backup_config() -> Result<serde_json::Value, String> {
    let config = read_sync_config();
    Ok(serde_json::json!({
        "auto_backup_before_sync": config.auto_backup_before_sync,
        "auto_backup_scheduled": config.auto_backup_scheduled,
        "auto_backup_interval_hours": config.auto_backup_interval_hours,
        "auto_backup_max_count": config.auto_backup_max_count,
    }))
}

#[tauri::command]
pub async fn set_auto_backup_config(
    before_sync: bool,
    scheduled: bool,
    interval_hours: u32,
    max_count: u32,
) -> Result<(), String> {
    if !(1..=24).contains(&interval_hours) {
        return Err("Interval must be 1-24 hours".to_string());
    }
    if !(1..=20).contains(&max_count) {
        return Err("Max count must be 1-20".to_string());
    }

    let mut config = read_sync_config();
    config.auto_backup_before_sync = before_sync;
    config.auto_backup_scheduled = scheduled;
    config.auto_backup_interval_hours = interval_hours;
    config.auto_backup_max_count = max_count;

    let path = crate::utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}
