use crate::network::transfer;
use crate::state::{AppState, FileInfo, Resolution, SyncAction, SyncPlan};
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

/// Set by `cancel_sync`; checked between files in `run_sync`. Only one sync
/// runs at a time in practice (clients pull from a single host).
static CANCEL_SYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ask the running sync to stop after the current file. Never interrupts a
/// file mid-transfer (the peer stream must stay in sync); the resume
/// checkpoint is kept so the next sync continues where this one stopped.
#[tauri::command]
pub async fn cancel_sync(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    let syncing = state.lock().await.is_any_syncing();
    if syncing {
        CANCEL_SYNC.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(syncing)
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
    // Diffing needs real hashes: a local manifest from a quick scan has empty
    // hashes, which would turn every file both sides have into a "conflict".
    let needs_rehash = {
        let app_state = state.lock().await;
        app_state.local_manifest.files.values().any(|f| f.hash.is_empty())
    };
    if needs_rehash {
        crate::commands::files::scan_files_inner(state.inner(), None, true).await?;
    }

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

    // Pull-only model: drop SendToRemote actions (uploads to the host aren't
    // supported by the transfer protocol). Clients download; the host serves.
    diff::retain_pull_only(&mut plan);

    // Filter out actions for content types disabled by folder permissions.
    // Permissions are keyed by content type ID; map each file through its
    // path + file_type (see AppState::is_file_info_allowed).
    {
        let app_state_ref = &*app_state;
        plan.actions.retain(|action| match action {
            SyncAction::SendToRemote(f) => app_state_ref.is_file_info_allowed(f),
            SyncAction::ReceiveFromRemote(f) => app_state_ref.is_file_info_allowed(f),
            SyncAction::Conflict { remote, .. } => app_state_ref.is_file_info_allowed(remote),
            SyncAction::Delete(_) => true,
        });
    }

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

    // Hash of the full plan, before resume filtering or conflict resolution.
    // It is stored on the plan and reused by run_sync for the checkpoint, so a
    // resumed (filtered) or conflict-resolved plan still matches its checkpoint
    // if the sync is interrupted again.
    let plan_hash = diff::compute_plan_hash(&plan);
    plan.plan_hash = Some(plan_hash.clone());

    // Check for resumable checkpoint
    let mut resumed_files: u64 = 0;
    if let Some(checkpoint) = read_checkpoint() {
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
        // A stale request from an earlier sync must not cancel this one.
        CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
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

/// Error returned by `execute_sync` when the user cancelled (the UI matches on it).
pub const SYNC_CANCELLED: &str = "Sync cancelled";

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
    let mut files_received = 0u64;
    let started = std::time::Instant::now();
    let state_arc = state.inner().clone();

    let plan_hash = plan
        .plan_hash
        .clone()
        .unwrap_or_else(|| diff::compute_plan_hash(plan));
    let game_id = {
        let app_state = state.lock().await;
        app_state.active_game.clone()
    };
    // When resuming, carry over the files already completed by the previous
    // attempt so a second interruption doesn't forget them.
    let previously_completed = read_checkpoint()
        .filter(|cp| cp.game == game_id && cp.peer_id == peer_id && cp.plan_hash == plan_hash)
        .map(|cp| cp.completed_files)
        .unwrap_or_default();
    let mut checkpoint = SyncCheckpoint {
        game: game_id,
        peer_id: peer_id.to_string(),
        plan_hash,
        completed_files: previously_completed,
        total_files,
        total_bytes: plan.total_bytes,
        started_at: crate::utils::timestamp_now(),
    };
    write_checkpoint(&checkpoint);
    let mut cancelled = false;

    for action in &plan.actions {
        if CANCEL_SYNC.swap(false, std::sync::atomic::Ordering::SeqCst) {
            cancelled = true;
            break;
        }
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
                let result = match plan.keep_both.get(&file_info.relative_path) {
                    Some(remote_path) => {
                        receive_keep_both(
                            &state_arc,
                            peer_id,
                            base_path,
                            remote_path,
                            &file_info.relative_path,
                        )
                        .await
                    }
                    None => {
                        transfer::request_file(
                            &state_arc,
                            peer_id,
                            &file_info.relative_path,
                            base_path,
                        )
                        .await
                    }
                };
                match result {
                    Ok(()) => {
                        files_done += 1;
                        files_received += 1;
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

    let duration_ms = started.elapsed().as_millis() as u64;

    if files_received > 0 && read_sync_config().clear_cache_after_sync {
        clear_post_sync_caches(state, app, base_path).await;
    }

    let _ = app.emit(
        "sync-complete",
        serde_json::json!({
            "files_synced": files_done,
            "total_bytes": plan.total_bytes,
            "errors": sync_errors,
            "peer_id": peer_id,
            "cancelled": cancelled,
        }),
    );

    // A cancelled sync keeps its checkpoint so the next plan resumes.
    if !cancelled {
        delete_checkpoint();
    }

    // Record sync history
    {
        let app_state = state.lock().await;
        let peer_name = app_state.connections.get(peer_id)
            .map(|c| c.info.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let game = app_state.active_game.clone();
        let has_receives = plan.actions.iter().any(|a| matches!(a, SyncAction::ReceiveFromRemote(_)));
        let has_sends = plan.actions.iter().any(|a| matches!(a, SyncAction::SendToRemote(_)));
        let direction = match (has_receives, has_sends) {
            (true, true) => "bidirectional",
            (true, false) => "received",
            (false, true) => "sent",
            _ => "none",
        }.to_string();
        append_sync_history(SyncHistoryEntry {
            timestamp: crate::utils::timestamp_now(),
            game,
            peer_name,
            files_synced: files_done,
            total_bytes: plan.total_bytes,
            errors: sync_errors.clone(),
            direction,
            duration_ms,
            cancelled,
        });
    }

    if cancelled {
        return Err(SYNC_CANCELLED.to_string());
    }
    if !sync_errors.is_empty() {
        return Err(format!("{} file(s) failed to sync", sync_errors.len()));
    }

    Ok(())
}

/// Delete the active game's registry `post_sync_delete` files (stale caches the
/// game would otherwise keep using, e.g. Sims 4 localthumbcache.package).
async fn clear_post_sync_caches(
    state: &tauri::State<'_, Arc<Mutex<AppState>>>,
    app: &tauri::AppHandle,
    base_path: &str,
) {
    let targets = {
        let app_state = state.lock().await;
        app_state
            .game_registry
            .games
            .iter()
            .find(|g| g.id == app_state.active_game)
            .map(|g| g.post_sync_delete.clone())
            .unwrap_or_default()
    };
    let mut deleted = Vec::new();
    for rel in targets {
        let Ok(full) = utils::safe_join(base_path, &rel) else {
            log::warn!("Rejected post-sync delete path: {}", rel);
            continue;
        };
        match tokio::fs::remove_file(&full).await {
            Ok(()) => deleted.push(rel),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Failed to clear cache {}: {}", rel, e),
        }
    }
    if !deleted.is_empty() {
        log::info!("Cleared game caches after sync: {:?}", deleted);
        let _ = app.emit("caches-cleared", serde_json::json!({ "files": deleted }));
    }
}

/// Download `remote_path` from the peer but save it locally as `local_dest`,
/// leaving the existing local file at `remote_path` untouched ("keep both").
///
/// The peer only serves files under their real path and `request_file` always
/// writes to that same relative path, so the local file is moved aside for
/// the duration of the download and put back afterwards.
async fn receive_keep_both(
    state: &Arc<Mutex<AppState>>,
    peer_id: &str,
    base_path: &str,
    remote_path: &str,
    local_dest: &str,
) -> Result<(), String> {
    let original = utils::safe_join(base_path, remote_path)?;
    let dest = utils::safe_join(base_path, local_dest)?;

    let mut aside_name = original
        .file_name()
        .ok_or("Invalid file name")?
        .to_os_string();
    aside_name.push(format!(".synccrate-keep-{}.tmp", utils::timestamp_now()));
    let aside = original.with_file_name(aside_name);

    let had_local = tokio::fs::try_exists(&original).await.unwrap_or(false);
    if had_local {
        tokio::fs::rename(&original, &aside)
            .await
            .map_err(|e| format!("Cannot move local copy aside: {}", e))?;
    }

    let restore_local = |aside: std::path::PathBuf, original: std::path::PathBuf| async move {
        if had_local {
            if let Err(e) = tokio::fs::rename(&aside, &original).await {
                log::error!(
                    "Failed to restore local file {} from {}: {}",
                    original.display(),
                    aside.display(),
                    e
                );
                return Err(format!(
                    "Local copy could not be restored; it was left at {}",
                    aside.display()
                ));
            }
        }
        Ok(())
    };

    if let Err(e) = transfer::request_file(state, peer_id, remote_path, base_path).await {
        // request_file writes via temp file + rename, so on failure `original`
        // was never replaced; just put the local copy back.
        restore_local(aside, original).await?;
        return Err(e);
    }

    // `original` now holds the remote copy: move it to its keep-both name,
    // then put the local copy back in place.
    let moved = tokio::fs::rename(&original, &dest).await;
    let restored = restore_local(aside, original).await;
    moved.map_err(|e| format!("Cannot save remote copy as {}: {}", local_dest, e))?;
    restored
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
    let local_has_path = app_state.local_manifest.files.contains_key(&path);

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
        let was_conflict = plan.actions.iter().any(|a| {
            matches!(a, SyncAction::Conflict { local, .. } if local.relative_path == path)
        });
        // A conflict that was already resolved earlier: drop the previous
        // resolution first so re-resolving (e.g. KeepBoth -> UseTheirs) doesn't
        // leave both downloads queued.
        let previously_kept_both: Vec<String> = plan
            .keep_both
            .iter()
            .filter(|(_, orig)| **orig == path)
            .map(|(renamed, _)| renamed.clone())
            .collect();
        let previously_resolved = !previously_kept_both.is_empty()
            || (local_has_path
                && plan.actions.iter().any(|a| {
                    matches!(a, SyncAction::ReceiveFromRemote(f) if f.relative_path == path)
                }));
        if !was_conflict && !previously_resolved {
            // Nothing to resolve for this path; leave the plan untouched.
            return Ok(plan.clone());
        }

        plan.actions.retain(|action| match action {
            SyncAction::Conflict { local, .. } => local.relative_path != path,
            SyncAction::ReceiveFromRemote(f) => {
                f.relative_path != path && !previously_kept_both.contains(&f.relative_path)
            }
            _ => true,
        });
        for renamed in &previously_kept_both {
            plan.keep_both.remove(renamed);
        }

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

                    // The peer doesn't have a file under the renamed path —
                    // remember which real path to request (see receive_keep_both).
                    plan.keep_both.insert(renamed.relative_path.clone(), path.clone());
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
                if keep_newer_resolution(local, remote) == Resolution::UseTheirs {
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

/// "Keep newer" decision for one conflict: the remote copy wins only when it
/// is strictly newer; local-newer and equal timestamps keep the local file.
pub(crate) fn keep_newer_resolution(local: &FileInfo, remote: &FileInfo) -> Resolution {
    if remote.modified > local.modified {
        Resolution::UseTheirs
    } else {
        Resolution::KeepMine
    }
}

// --- Selective Sync helpers ---

pub(crate) fn glob_matches(pattern: &str, path: &str) -> bool {
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SyncConfig {
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub auto_backup_before_sync: bool,
    #[serde(default)]
    pub auto_backup_scheduled: bool,
    #[serde(default = "default_backup_interval")]
    pub auto_backup_interval_hours: u32,
    #[serde(default = "default_backup_max_count")]
    pub auto_backup_max_count: u32,
    /// Transfer speed limit in bytes/sec. 0 = unlimited.
    #[serde(default)]
    pub transfer_speed_limit: u64,
    /// Delete the game's registry `post_sync_delete` files (e.g. Sims 4
    /// localthumbcache.package) after a sync that received files.
    #[serde(default = "default_true")]
    pub clear_cache_after_sync: bool,
    /// Hide to the system tray instead of quitting when the main window is closed.
    #[serde(default)]
    pub close_to_tray: bool,
}

fn default_true() -> bool { true }
fn default_backup_interval() -> u32 { 4 }
fn default_backup_max_count() -> u32 { 5 }

// Manual Default so it agrees with the serde defaults. The derived Default gave
// interval 0 / max_count 0, which was persisted by any setter that started from
// a missing config file (e.g. set_exclude_patterns) — max_count 0 makes
// prune_auto_backups delete every auto-backup, including the one just created.
impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: Vec::new(),
            auto_backup_before_sync: false,
            auto_backup_scheduled: false,
            auto_backup_interval_hours: default_backup_interval(),
            auto_backup_max_count: default_backup_max_count(),
            transfer_speed_limit: 0,
            clear_cache_after_sync: true,
            close_to_tray: false,
        }
    }
}

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

pub(crate) fn read_exclude_patterns() -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matches_wildcard_all() {
        assert!(glob_matches("*", "anything/goes/here.txt"));
    }

    #[test]
    fn test_glob_matches_extension() {
        assert!(glob_matches("*.ts4script", "Mods/drama.ts4script"));
        assert!(glob_matches("*.package", "Mods/sub/cc.package"));
        assert!(!glob_matches("*.ts4script", "Mods/cc.package"));
    }

    #[test]
    fn test_glob_matches_prefix() {
        assert!(glob_matches("Mods/*", "Mods/file.package"));
        assert!(glob_matches("Mods/*", "Mods/sub/deep.package"));
        assert!(!glob_matches("Mods/*", "Saves/game.save"));
    }

    #[test]
    fn test_glob_matches_exact() {
        assert!(glob_matches("Mods/specific.package", "Mods/specific.package"));
        assert!(!glob_matches("Mods/specific.package", "Mods/other.package"));
    }

    #[test]
    fn test_glob_matches_backslash_normalization() {
        assert!(glob_matches("Mods\\*", "Mods/file.package"));
        assert!(glob_matches("Mods/*", "Mods\\file.package"));
    }

    // --- SyncConfig tests ---

    #[test]
    fn test_sync_config_default_has_zero_speed_limit() {
        let config = SyncConfig::default();
        assert_eq!(config.transfer_speed_limit, 0);
        assert!(!config.auto_backup_before_sync);
        assert!(!config.auto_backup_scheduled);
        // Default must agree with the serde defaults (never 0 — see impl Default)
        assert_eq!(config.auto_backup_interval_hours, 4);
        assert_eq!(config.auto_backup_max_count, 5);
    }

    #[test]
    fn test_sync_config_default_matches_serde_defaults() {
        let from_empty: SyncConfig = serde_json::from_str(r#"{"exclude_patterns": []}"#).unwrap();
        let def = SyncConfig::default();
        assert_eq!(from_empty.auto_backup_interval_hours, def.auto_backup_interval_hours);
        assert_eq!(from_empty.auto_backup_max_count, def.auto_backup_max_count);
    }

    #[test]
    fn test_sync_config_roundtrip_with_speed_limit() {
        let config = SyncConfig {
            exclude_patterns: vec!["*.tmp".to_string()],
            auto_backup_before_sync: true,
            auto_backup_scheduled: false,
            auto_backup_interval_hours: 4,
            auto_backup_max_count: 5,
            transfer_speed_limit: 10_485_760, // 10 MB/s
            clear_cache_after_sync: false,
            close_to_tray: true,
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let parsed: SyncConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.transfer_speed_limit, 10_485_760);
        assert_eq!(parsed.exclude_patterns, vec!["*.tmp"]);
        assert!(parsed.auto_backup_before_sync);
    }

    #[test]
    fn test_sync_config_missing_speed_limit_defaults_to_zero() {
        // Simulates loading a config file from before speed limit was added
        let json = r#"{"exclude_patterns": [], "auto_backup_before_sync": false}"#;
        let config: SyncConfig = serde_json::from_str(json).expect("deserialize");
        assert_eq!(config.transfer_speed_limit, 0);
    }

    // --- SyncHistoryEntry tests ---

    #[test]
    fn test_sync_history_entry_roundtrip() {
        let entry = SyncHistoryEntry {
            timestamp: 1711548000,
            game: "sims4".to_string(),
            peer_name: "Alice".to_string(),
            files_synced: 42,
            total_bytes: 1073741824,
            errors: vec![],
            direction: "received".to_string(),
            duration_ms: 0,
            cancelled: false,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let parsed: SyncHistoryEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.game, "sims4");
        assert_eq!(parsed.peer_name, "Alice");
        assert_eq!(parsed.files_synced, 42);
        assert_eq!(parsed.total_bytes, 1073741824);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.direction, "received");
    }

    #[test]
    fn test_sync_history_entry_with_errors() {
        let entry = SyncHistoryEntry {
            timestamp: 1711548000,
            game: "terraria".to_string(),
            peer_name: "Bob".to_string(),
            files_synced: 10,
            total_bytes: 5000,
            errors: vec!["Mods/broken.tmod: hash mismatch".to_string()],
            direction: "bidirectional".to_string(),
            duration_ms: 0,
            cancelled: false,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let parsed: SyncHistoryEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.errors.len(), 1);
        assert!(parsed.errors[0].contains("hash mismatch"));
    }

    #[test]
    fn test_sync_config_clear_cache_defaults_true() {
        let config: SyncConfig = serde_json::from_str(r#"{"exclude_patterns": []}"#).unwrap();
        assert!(config.clear_cache_after_sync);
        assert!(SyncConfig::default().clear_cache_after_sync);
    }

    #[test]
    fn test_sync_history_entry_missing_duration_defaults_zero() {
        let json = r#"{"timestamp":1,"game":"sims4","peer_name":"A","files_synced":1,"total_bytes":10,"errors":[],"direction":"received"}"#;
        let parsed: SyncHistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.duration_ms, 0);
    }

    fn hist(bytes: u64, ms: u64) -> SyncHistoryEntry {
        SyncHistoryEntry {
            timestamp: 0,
            game: "sims4".to_string(),
            peer_name: "A".to_string(),
            files_synced: 1,
            total_bytes: bytes,
            errors: vec![],
            direction: "received".to_string(),
            duration_ms: ms,
            cancelled: false,
        }
    }

    #[test]
    fn test_keep_newer_resolution() {
        let f = |modified: u64| FileInfo {
            relative_path: "Mods/a.package".into(),
            size: 1,
            hash: "h".into(),
            modified,
            file_type: "Mod".into(),
        };
        assert_eq!(keep_newer_resolution(&f(200), &f(100)), Resolution::KeepMine);
        assert_eq!(keep_newer_resolution(&f(100), &f(200)), Resolution::UseTheirs);
        assert_eq!(keep_newer_resolution(&f(100), &f(100)), Resolution::KeepMine);
    }

    #[test]
    fn test_typical_speed_none_without_usable_history() {
        assert_eq!(typical_speed(&[], 5), None);
        assert_eq!(typical_speed(&[hist(100, 0), hist(0, 1000)], 5), None);
    }

    #[test]
    fn test_typical_speed_median_of_recent() {
        // 1000 B/s, 3000 B/s, 2000 B/s -> median 2000
        let h = vec![hist(1000, 1000), hist(3000, 1000), hist(2000, 1000)];
        assert_eq!(typical_speed(&h, 5), Some(2000));
        // Even count averages the middle two
        let h = vec![hist(1000, 1000), hist(3000, 1000)];
        assert_eq!(typical_speed(&h, 5), Some(2000));
    }

    #[test]
    fn test_typical_speed_uses_only_last_n() {
        // Oldest entries are slow; only the last 2 (fast) should count.
        let h = vec![hist(1, 1000), hist(1, 1000), hist(1, 1000), hist(8000, 1000), hist(8000, 1000)];
        assert_eq!(typical_speed(&h, 2), Some(8000));
        // Unusable entries are skipped, not counted toward the sample.
        let h = vec![hist(4000, 1000), hist(100, 0)];
        assert_eq!(typical_speed(&h, 1), Some(4000));
    }

    // --- SyncCheckpoint tests ---

    #[test]
    fn test_sync_checkpoint_roundtrip() {
        let cp = SyncCheckpoint {
            game: "sims4".to_string(),
            peer_id: "abc-123".to_string(),
            plan_hash: "deadbeef".to_string(),
            completed_files: vec!["Mods/a.package".to_string(), "Mods/b.package".to_string()],
            total_files: 10,
            total_bytes: 50000,
            started_at: 1711548000,
        };
        let json = serde_json::to_string(&cp).expect("serialize");
        let parsed: SyncCheckpoint = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.completed_files.len(), 2);
        assert_eq!(parsed.plan_hash, "deadbeef");
        assert_eq!(parsed.total_files, 10);
    }
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

// --- Transfer speed limit ---

#[tauri::command]
pub async fn get_transfer_speed_limit() -> Result<u64, String> {
    Ok(read_sync_config().transfer_speed_limit)
}

#[tauri::command]
pub async fn set_transfer_speed_limit(limit: u64) -> Result<(), String> {
    let mut config = read_sync_config();
    config.transfer_speed_limit = limit;
    let path = crate::utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_clear_cache_after_sync() -> Result<bool, String> {
    Ok(read_sync_config().clear_cache_after_sync)
}

#[tauri::command]
pub async fn set_clear_cache_after_sync(enabled: bool) -> Result<(), String> {
    let mut config = read_sync_config();
    config.clear_cache_after_sync = enabled;
    let path = crate::utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

/// Read the current transfer speed limit (called from transfer layer).
pub fn get_speed_limit() -> u64 {
    read_sync_config().transfer_speed_limit
}

// --- Sync history ---

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SyncHistoryEntry {
    pub timestamp: u64,
    pub game: String,
    pub peer_name: String,
    pub files_synced: u64,
    pub total_bytes: u64,
    pub errors: Vec<String>,
    pub direction: String,
    /// Wall-clock transfer time; 0 for entries recorded before this existed.
    #[serde(default)]
    pub duration_ms: u64,
    /// The user stopped the sync before it finished.
    #[serde(default)]
    pub cancelled: bool,
}

fn sync_history_path() -> std::path::PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    config.join("synccrate").join("sync_history.json")
}

pub fn append_sync_history(entry: SyncHistoryEntry) {
    let path = sync_history_path();
    let mut history: Vec<SyncHistoryEntry> = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    history.push(entry);
    // Keep last 100 entries
    if history.len() > 100 {
        history = history.split_off(history.len() - 100);
    }
    if let Ok(data) = serde_json::to_string_pretty(&history) {
        let _ = std::fs::write(&path, data);
    }
}

#[tauri::command]
pub async fn get_sync_history() -> Result<Vec<SyncHistoryEntry>, String> {
    let path = sync_history_path();
    if path.exists() {
        let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let history: Vec<SyncHistoryEntry> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(history)
    } else {
        Ok(Vec::new())
    }
}

/// Median bytes/sec over the most recent `sample` history entries that have a
/// measured duration and moved data. `None` when there is no usable history.
pub(crate) fn typical_speed(history: &[SyncHistoryEntry], sample: usize) -> Option<u64> {
    let mut speeds: Vec<u64> = history
        .iter()
        .rev()
        .filter(|e| e.duration_ms > 0 && e.total_bytes > 0)
        .take(sample)
        .map(|e| (e.total_bytes as u128 * 1000 / e.duration_ms as u128) as u64)
        .collect();
    if speeds.is_empty() {
        return None;
    }
    speeds.sort_unstable();
    let mid = speeds.len() / 2;
    Some(if speeds.len() % 2 == 0 {
        (speeds[mid - 1] + speeds[mid]) / 2
    } else {
        speeds[mid]
    })
}

/// Typical transfer speed (bytes/sec) from recent syncs, for the pre-sync estimate.
#[tauri::command]
pub async fn get_typical_transfer_speed() -> Result<Option<u64>, String> {
    let history = get_sync_history().await.unwrap_or_default();
    Ok(typical_speed(&history, 5))
}

#[tauri::command]
pub async fn clear_sync_history() -> Result<(), String> {
    let path = sync_history_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
