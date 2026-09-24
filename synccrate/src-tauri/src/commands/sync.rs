use crate::network::transfer;
use crate::state::{AppState, ConflictPair, FileInfo, ReplaceTarget, Resolution, SyncAction, SyncPlan};
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

const SYNC_RUNNING: &str = "A sync is already running — wait for it to finish";

#[tauri::command]
pub async fn compute_sync_plan(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    peer_id: Option<String>,
) -> Result<SyncPlan, String> {
    // Diffing needs real hashes: a local manifest from a quick scan has empty
    // hashes, which would turn every file both sides have into a "conflict".
    // An empty manifest may simply never have been scanned (or was dropped on
    // a path change); diffing it would make every host file look new.
    let needs_rehash = {
        let app_state = state.lock().await;
        if app_state.is_any_syncing() {
            return Err(SYNC_RUNNING.to_string());
        }
        let files = &app_state.local_manifest.files;
        files.is_empty() || files.values().any(|f| f.hash.is_empty())
    };
    if needs_rehash {
        crate::commands::files::scan_files_inner(state.inner(), None, true).await?;
    }

    let mut app_state = state.lock().await;
    // A sync may have started while we were scanning; replacing its plan now
    // would desync the progress UI from what's actually running.
    if app_state.is_any_syncing() {
        return Err(SYNC_RUNNING.to_string());
    }

    let resolved_id = app_state.resolve_peer_id(peer_id)?;
    let active_game = app_state.active_game.clone();
    let base_path = app_state.active_game_path()?;
    let content_types = crate::commands::files::get_game_def(&app_state.game_registry, &active_game)
        .map(|g| g.content_types.clone())
        .ok_or_else(|| format!("Game '{}' not found in registry", active_game))?;

    let conn = app_state
        .connections
        .get(&resolved_id)
        .ok_or("Peer not found")?;

    let mut remote = conn
        .remote_manifest
        .clone()
        .ok_or("No remote manifest available. Connect to a peer first.")?;
    let host_game = conn.info.game_id.clone();

    // Never plan downloads outside this game's content folders, whatever the
    // host sends (hosts older than 0.5.6 don't say which game they share).
    let remote_total = remote.files.len();
    let skipped_foreign = diff::drop_foreign(&mut remote, &content_types);

    let mut plan = diff::compute_diff(&app_state.local_manifest, &remote);
    plan.game_id = active_game;
    plan.base_path = base_path;
    plan.skipped_foreign = skipped_foreign;
    plan.warning = diff::foreign_warning(host_game.as_deref(), skipped_foreign, remote_total);
    plan.host_game = host_game;
    if skipped_foreign > 0 {
        log::warn!("Skipped {} host file(s) outside this game's content folders", skipped_foreign);
    }

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

    // Hash of the full plan, before conflict resolution. It is stored on the
    // plan and reused by run_sync for the checkpoint, so a conflict-resolved
    // plan still matches its checkpoint if the sync is interrupted again.
    let plan_hash = diff::compute_plan_hash(&plan);
    plan.plan_hash = Some(plan_hash.clone());

    // Resume: files a previous (interrupted) attempt completed are no longer
    // dropped from the plan on the checkpoint's word. That trusted a list
    // without checking the files, so a file deleted or changed since was
    // silently skipped. Instead each receive checks its destination first and
    // skips the download when the file is already there with the expected
    // hash (see transfer::receive_file). The count is informational.
    let mut resumed_files: u64 = 0;
    if let Some(checkpoint) = read_checkpoint() {
        if checkpoint.game == plan.game_id
            && checkpoint.peer_id == resolved_id
            && checkpoint.plan_hash == plan_hash
            && !checkpoint.completed_files.is_empty()
        {
            let completed: std::collections::HashSet<&str> =
                checkpoint.completed_files.iter().map(|s| s.as_str()).collect();
            resumed_files = plan
                .actions
                .iter()
                .filter(|a| match a {
                    SyncAction::ReceiveFromRemote(f) => completed.contains(f.relative_path.as_str()),
                    _ => false,
                })
                .count() as u64;
            log::info!("Resuming sync: {} files may already be complete", resumed_files);
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
    let (plan, base_path, resolved_id, content_types) = {
        let mut app_state = state.lock().await;
        let resolved_id = app_state.resolve_peer_id(peer_id)?;
        let base = app_state.active_game_path()?;
        let active_game = app_state.active_game.clone();
        let content_types = crate::commands::files::get_game_def(&app_state.game_registry, &active_game)
            .map(|g| g.content_types.clone())
            .unwrap_or_default();
        if app_state.is_any_syncing() {
            return Err("Sync is already in progress".to_string());
        }

        let conn = app_state
            .connections
            .get_mut(&resolved_id)
            .ok_or("Peer not found")?;

        let plan = conn.sync_plan.take().ok_or("No sync plan computed.")?;

        // The plan was computed against one game folder; the active game or its
        // path may have changed since. Drop the plan rather than run it here.
        if let Some(msg) = diff::plan_target_mismatch(&plan, &active_game, &base) {
            return Err(msg);
        }

        let has_conflicts = plan.actions.iter().any(|a| matches!(a, SyncAction::Conflict { .. }));
        if has_conflicts {
            conn.sync_plan = Some(plan);
            return Err("Resolve all conflicts before syncing".to_string());
        }

        conn.is_syncing = true;
        // A stale request from an earlier sync must not cancel this one.
        CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
        (plan, base, resolved_id, content_types)
    };

    {
        let base = base_path.clone();
        let recovered = tokio::task::spawn_blocking(move || recover_keep_temps(&base, &content_types))
            .await
            .unwrap_or(0);
        if recovered > 0 {
            log::warn!("Restored {} local file(s) left aside by an interrupted keep-both", recovered);
        }
    }

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

/// Put back local files that an interrupted "keep both" (in versions that
/// moved the local copy aside while downloading) left as
/// `<name>.synccrate-keep-<ts>.tmp`, when `<name>` is missing. Only walks the
/// game's content folders (never follows links); returns how many were restored.
fn recover_keep_temps(base: &str, content_types: &[crate::registry::ContentType]) -> usize {
    let mut restored = 0;
    for ct in content_types {
        let dir = if ct.folder.is_empty() || ct.folder == "." {
            std::path::PathBuf::from(base)
        } else {
            std::path::Path::new(base).join(&ct.folder)
        };
        if !dir.is_dir() {
            continue;
        }
        let mut walker = walkdir::WalkDir::new(&dir).follow_links(false);
        if !ct.recursive {
            walker = walker.max_depth(1);
        }
        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if entry.path_is_symlink() || !entry.file_type().is_file() {
                continue;
            }
            let Some(original) = entry.file_name().to_str().and_then(diff::keep_tmp_original) else {
                continue;
            };
            let target = entry.path().with_file_name(original);
            if target.exists() {
                log::warn!(
                    "Leftover keep-both temp {} kept: {} exists",
                    entry.path().display(),
                    target.display()
                );
            } else if std::fs::rename(entry.path(), &target).is_ok() {
                restored += 1;
            }
        }
    }
    restored
}

/// Which host file to request, where to write it and what it may replace, for
/// one `ReceiveFromRemote` of the plan.
pub(crate) fn receive_target(plan: &SyncPlan, file: &FileInfo) -> (String, String, transfer::ReplacePolicy) {
    // Keep-both: the action carries the new local name; the host only knows the
    // real path. The name was chosen to exist neither locally nor on the host.
    if let Some(remote_path) = plan.keep_both.get(&file.relative_path) {
        return (
            remote_path.clone(),
            file.relative_path.clone(),
            transfer::ReplacePolicy::MustNotExist,
        );
    }
    if let Some(target) = plan.use_theirs.get(&file.relative_path) {
        return (
            file.relative_path.clone(),
            target.local_path.clone(),
            transfer::ReplacePolicy::ReplaceIfHash(target.local_hash.clone()),
        );
    }
    (
        file.relative_path.clone(),
        file.relative_path.clone(),
        transfer::ReplacePolicy::MustNotExist,
    )
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
                let (remote_path, local_path, policy) = receive_target(plan, file_info);
                let result = transfer::receive_file(
                    &state_arc,
                    peer_id,
                    base_path,
                    transfer::ReceiveRequest {
                        remote_path: &remote_path,
                        local_path: &local_path,
                        expected_hash: &file_info.hash,
                        policy,
                    },
                )
                .await;
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

/// Apply (or change) the resolution of the conflict whose local path is
/// `local_path`. Returns false when there is no such conflict.
///
/// The conflict pair is remembered in `resolved_conflicts`, so re-resolving
/// (e.g. KeepBoth -> UseTheirs) first removes the earlier resolution's
/// download instead of queueing both. `taken` says whether a keep-both name
/// collides with an existing local or host file (by match key).
pub(crate) fn apply_resolution(
    plan: &mut SyncPlan,
    local_path: &str,
    resolution: Resolution,
    taken: impl Fn(&str) -> bool,
) -> Result<bool, String> {
    let pair = plan
        .actions
        .iter()
        .find_map(|a| match a {
            SyncAction::Conflict { local, remote } if local.relative_path == local_path => {
                Some(ConflictPair { local: local.clone(), remote: remote.clone() })
            }
            _ => None,
        })
        .or_else(|| plan.resolved_conflicts.get(local_path).cloned());
    let Some(pair) = pair else { return Ok(false) };
    let remote_path = pair.remote.relative_path.clone();

    // Undo an earlier resolution of this conflict.
    let old_keep_both: Vec<String> = plan
        .keep_both
        .iter()
        .filter(|(_, orig)| **orig == remote_path)
        .map(|(renamed, _)| renamed.clone())
        .collect();
    let had_use_theirs = plan
        .use_theirs
        .get(&remote_path)
        .map_or(false, |t| t.local_path == local_path);
    plan.actions.retain(|action| match action {
        SyncAction::Conflict { local, .. } => local.relative_path != local_path,
        SyncAction::ReceiveFromRemote(f) => {
            !old_keep_both.contains(&f.relative_path) && !(had_use_theirs && f.relative_path == remote_path)
        }
        _ => true,
    });
    for renamed in &old_keep_both {
        plan.keep_both.remove(renamed);
    }
    if had_use_theirs {
        plan.use_theirs.remove(&remote_path);
    }

    match resolution {
        Resolution::KeepMine => {}
        Resolution::UseTheirs => {
            // Written to the *local* path (keeps the user's case and disabled
            // state) and only if the local file is unchanged since the compare.
            plan.use_theirs.insert(
                remote_path.clone(),
                ReplaceTarget {
                    local_path: local_path.to_string(),
                    local_hash: pair.local.hash.clone(),
                },
            );
            plan.actions.push(SyncAction::ReceiveFromRemote(pair.remote.clone()));
        }
        Resolution::KeepBoth => {
            let pending: std::collections::HashSet<String> =
                plan.keep_both.keys().map(|k| diff::match_key(k)).collect();
            let name = diff::keep_both_name(local_path, |c| {
                taken(c) || pending.contains(&diff::match_key(c))
            })
            .ok_or_else(|| format!("No free name to keep both copies of {}", local_path))?;
            let mut renamed = pair.remote.clone();
            renamed.relative_path = name.clone();
            // The host has no file under the new name — remember which real
            // path to request (see receive_target).
            plan.keep_both.insert(name, remote_path);
            plan.actions.push(SyncAction::ReceiveFromRemote(renamed));
        }
    }
    plan.resolved_conflicts.insert(local_path.to_string(), pair);
    Ok(true)
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
    let local_keys: std::collections::HashSet<String> = app_state
        .local_manifest
        .files
        .keys()
        .map(|k| diff::match_key(k))
        .collect();

    let conn = app_state
        .connections
        .get_mut(&resolved_id)
        .ok_or("Peer not found")?;

    if conn.is_syncing {
        return Err("Cannot resolve conflicts while sync is in progress".to_string());
    }

    let remote_keys: std::collections::HashSet<String> = conn
        .remote_manifest
        .as_ref()
        .map(|m| m.files.keys().map(|k| diff::match_key(k)).collect())
        .unwrap_or_default();

    let plan = conn.sync_plan.as_mut().ok_or("No sync plan available")?;
    let base_path = plan.base_path.clone();
    apply_resolution(plan, &path, resolution, |candidate| {
        let key = diff::match_key(candidate);
        local_keys.contains(&key)
            || remote_keys.contains(&key)
            || utils::safe_join(&base_path, candidate).map_or(true, |p| p.exists())
    })?;
    Ok(plan.clone())
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

    let plan = conn.sync_plan.as_mut().ok_or("No sync plan available")?;
    let decisions: Vec<(String, Resolution)> = plan
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncAction::Conflict { local, remote } => {
                Some((local.relative_path.clone(), keep_newer_resolution(local, remote)))
            }
            _ => None,
        })
        .collect();
    for (path, resolution) in decisions {
        // Keep-mine / use-theirs never pick a new name, so nothing is "taken".
        apply_resolution(plan, &path, resolution, |_| true)?;
    }
    Ok(plan.clone())
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

    // --- Conflict resolution / receive targets ---

    fn fi(path: &str, hash: &str) -> FileInfo {
        FileInfo {
            relative_path: path.into(),
            size: 1,
            hash: hash.into(),
            modified: 0,
            file_type: "Mod".into(),
        }
    }

    fn conflict_plan(local: &str, remote: &str) -> SyncPlan {
        SyncPlan {
            actions: vec![SyncAction::Conflict { local: fi(local, "mine"), remote: fi(remote, "theirs") }],
            game_id: "sims4".into(),
            base_path: "C:/Game".into(),
            ..Default::default()
        }
    }

    fn receives(plan: &SyncPlan) -> Vec<String> {
        plan.actions
            .iter()
            .filter_map(|a| match a {
                SyncAction::ReceiveFromRemote(f) => Some(f.relative_path.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn use_theirs_writes_to_the_local_disabled_path_if_unchanged() {
        let mut plan = conflict_plan("Mods/x.package.disabled", "Mods/x.package");
        assert!(apply_resolution(&mut plan, "Mods/x.package.disabled", Resolution::UseTheirs, |_| false).unwrap());
        assert_eq!(receives(&plan), vec!["Mods/x.package"]);
        let recv = match &plan.actions[0] { SyncAction::ReceiveFromRemote(f) => f.clone(), _ => unreachable!() };
        let (remote, local, policy) = receive_target(&plan, &recv);
        assert_eq!(remote, "Mods/x.package");
        assert_eq!(local, "Mods/x.package.disabled");
        assert_eq!(policy, transfer::ReplacePolicy::ReplaceIfHash("mine".into()));
    }

    #[test]
    fn plain_receive_never_replaces() {
        let plan = SyncPlan::default();
        let (remote, local, policy) = receive_target(&plan, &fi("Mods/a.package", "h"));
        assert_eq!((remote.as_str(), local.as_str()), ("Mods/a.package", "Mods/a.package"));
        assert_eq!(policy, transfer::ReplacePolicy::MustNotExist);
    }

    #[test]
    fn keep_both_picks_a_free_name_and_re_resolving_replaces_it() {
        let mut plan = conflict_plan("Mods/x.package", "Mods/x.package");
        // `x_remote.package` exists on the host: using it would hijack that download.
        let taken = |c: &str| crate::sync::diff::match_key(c) == "mods/x_remote.package";
        apply_resolution(&mut plan, "Mods/x.package", Resolution::KeepBoth, taken).unwrap();
        assert_eq!(receives(&plan), vec!["Mods/x_remote2.package"]);
        let recv = fi("Mods/x_remote2.package", "theirs");
        let (remote, local, policy) = receive_target(&plan, &recv);
        assert_eq!((remote.as_str(), local.as_str()), ("Mods/x.package", "Mods/x_remote2.package"));
        assert_eq!(policy, transfer::ReplacePolicy::MustNotExist);

        // Change of mind: only the new resolution's download stays queued.
        apply_resolution(&mut plan, "Mods/x.package", Resolution::UseTheirs, taken).unwrap();
        assert_eq!(receives(&plan), vec!["Mods/x.package"]);
        assert!(plan.keep_both.is_empty());
        apply_resolution(&mut plan, "Mods/x.package", Resolution::KeepMine, taken).unwrap();
        assert!(plan.actions.is_empty());
        assert!(plan.use_theirs.is_empty());
        assert!(!apply_resolution(&mut plan, "Mods/other.package", Resolution::KeepMine, taken).unwrap());
    }

    #[test]
    fn keep_both_suffix_goes_before_package_disabled() {
        let mut plan = conflict_plan("Mods/x.package.disabled", "Mods/x.package");
        apply_resolution(&mut plan, "Mods/x.package.disabled", Resolution::KeepBoth, |_| false).unwrap();
        assert_eq!(receives(&plan), vec!["Mods/x_remote.package.disabled"]);
    }

    #[test]
    fn recover_keep_temps_restores_missing_originals_only() {
        let base = std::env::temp_dir().join(format!("synccrate-keep-{}", uuid::Uuid::new_v4()));
        let mods = base.join("Mods").join("CC");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.package.synccrate-keep-123.tmp"), b"mine").unwrap();
        std::fs::write(mods.join("b.package"), b"current").unwrap();
        std::fs::write(mods.join("b.package.synccrate-keep-456.tmp"), b"old").unwrap();
        let def = crate::registry::load_registry().games.into_iter().find(|g| g.id == "sims4").unwrap();
        let restored = recover_keep_temps(&base.to_string_lossy(), &def.content_types);
        assert_eq!(restored, 1);
        assert_eq!(std::fs::read(mods.join("a.package")).unwrap(), b"mine");
        assert_eq!(std::fs::read(mods.join("b.package")).unwrap(), b"current");
        assert!(mods.join("b.package.synccrate-keep-456.tmp").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn dangerous_extension_sees_through_disabled_suffix() {
        assert!(utils::is_dangerous_extension("Mods/evil.exe"));
        assert!(utils::is_dangerous_extension("Mods/evil.EXE.disabled"));
        assert!(!utils::is_dangerous_extension("Mods/hair.package.disabled"));
        assert!(!utils::is_dangerous_extension("Mods/README"));
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
