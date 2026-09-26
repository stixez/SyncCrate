//! "Stay in sync": while connected, a client can pull the host's *new* files
//! automatically. It only ever adds files. Anything that would replace or
//! delete a local file, or needs a decision (conflicts), waits for the user
//! like before, and so do script files (`.dll`, `.ts4script`, …), which the
//! manual flow warns about before they're synced. It runs the normal
//! `execute_sync` pipeline, so undo, history and progress work unchanged.
use crate::state::{AppState, SessionType, SyncAction, SyncPlan};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Rescan the local folder before the next pull (at start, and after a pull).
static NEEDS_RESCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Unattended pulls stop here; anything bigger waits for a manual sync, so a
/// host can't fill the disk while nobody's watching.
const MAX_AUTO_PULL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Always treated as scripts, on top of the game's `dangerous_script_extensions`.
const SCRIPT_EXTENSIONS: &[&str] = &["dll", "exe", "ts4script", "lua", "jar", "js", "py", "bat", "cmd", "ps1", "asi", "so", "dylib"];

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AutoPullResult {
    /// New files downloaded.
    pub pulled: usize,
    /// New script files left for a manual sync.
    pub scripts_held: usize,
    /// Changes that need the user: conflicts, replacements, deletions.
    pub needs_review: usize,
    /// Why nothing ran, if nothing did (shown in the UI).
    pub skipped: Option<String>,
}

/// The part of a freshly computed plan that's safe to run unattended, plus
/// what was held back. Pure.
pub(crate) fn safe_subset(plan: &SyncPlan, script_exts: &[String]) -> (SyncPlan, usize, usize) {
    let is_script = |path: &str| {
        let ext = crate::commands::files::effective_extension(std::path::Path::new(path));
        SCRIPT_EXTENSIONS.contains(&ext.as_str()) || script_exts.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
    };
    let mut subset = SyncPlan { game_id: plan.game_id.clone(), base_path: plan.base_path.clone(), host_game: plan.host_game.clone(), auto_pull: true, ..Default::default() };
    let (mut scripts, mut review) = (0, 0);
    let excluded: std::collections::HashSet<&str> = plan.excluded.iter().map(String::as_str).collect();
    let kept_both: std::collections::HashSet<&str> = plan.keep_both.values().map(String::as_str).collect();
    for action in &plan.actions {
        match action {
            // The user excluded these; they stay out either way.
            SyncAction::ReceiveFromRemote(f) if excluded.contains(f.relative_path.as_str()) => {}
            SyncAction::Delete(p) if excluded.contains(p.as_str()) => {}
            SyncAction::ReceiveFromRemote(f)
                if !plan.use_theirs.contains_key(&f.relative_path) && !kept_both.contains(f.relative_path.as_str()) =>
            {
                if is_script(&f.relative_path) {
                    scripts += 1;
                } else {
                    subset.total_bytes += f.size;
                    subset.actions.push(action.clone());
                }
            }
            SyncAction::SendToRemote(_) => {}
            _ => review += 1,
        }
    }
    if subset.total_bytes > MAX_AUTO_PULL_BYTES {
        review += subset.actions.len();
        subset.actions.clear();
        subset.total_bytes = 0;
    }
    subset.plan_hash = Some(crate::sync::diff::compute_plan_hash(&subset));
    (subset, scripts, review)
}

#[tauri::command]
pub async fn auto_pull(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle) -> Result<AutoPullResult, String> {
    auto_pull_inner(state.inner(), crate::event_sink::from_app(&app)).await
}

pub(crate) async fn auto_pull_inner(state: &Arc<Mutex<AppState>>, events: crate::event_sink::Events) -> Result<AutoPullResult, String> {
    let skip = |why: &str| Ok(AutoPullResult { skipped: Some(why.to_string()), ..Default::default() });
    let (peer_id, procs, script_exts) = {
        let s = state.lock().await;
        if s.session_type != SessionType::Client {
            return skip("Not connected to a host.");
        }
        if s.is_any_syncing() || crate::commands::backup::restore_in_progress() {
            return skip("A sync or restore is running.");
        }
        let Some(id) = s.connections.keys().next().cloned() else { return skip("Not connected to a host.") };
        if s.connections.get(&id).is_some_and(|c| c.sync_plan.as_ref().is_some_and(|p| !p.actions.is_empty())) {
            // The user is looking at a plan; don't replace it under them.
            return skip("A sync plan is open.");
        }
        let def = s.game_registry.games.iter().find(|g| g.id == s.active_game);
        (
            id,
            def.map(|g| g.process_names.clone()).unwrap_or_default(),
            def.map(|g| g.dangerous_script_extensions.clone()).unwrap_or_default(),
        )
    };
    // New mods appearing mid-game can break a running game; wait for it to close.
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return skip("The game is running.");
    }

    // Our own manifest must be current after a pull (the frontend usually
    // rescans after a sync, but nothing does between unattended pulls), or
    // the files we just pulled look missing again. Rescanning every minute
    // regardless walked big Mods folders for nothing; a stale entry for a
    // file the user added themselves is harmless (the download finds it
    // already there and writes nothing).
    if NEEDS_RESCAN.swap(false, std::sync::atomic::Ordering::SeqCst) {
        if let Err(e) = crate::commands::files::scan_files_inner(state, None, true).await {
            NEEDS_RESCAN.store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(e);
        }
    }
    crate::network::transfer::refresh_remote_manifest(state, &peer_id).await?;
    let plan = crate::commands::sync::compute_sync_plan_inner(state, Some(peer_id.clone())).await?;
    let (subset, scripts_held, needs_review) = safe_subset(&plan, &script_exts);
    let pulled = subset.actions.len();
    {
        let mut s = state.lock().await;
        let Some(conn) = s.connections.get_mut(&peer_id) else { return skip("Disconnected.") };
        if plan.warning.is_some() {
            // e.g. an old host that seems to share another game: never unattended.
            conn.sync_plan = None;
            return Ok(AutoPullResult { needs_review: needs_review + pulled, scripts_held, skipped: plan.warning.clone(), pulled: 0 });
        }
        conn.sync_plan = (pulled > 0).then_some(subset);
    }
    if pulled > 0 {
        NEEDS_RESCAN.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::commands::sync::execute_sync_inner(state, events, Some(peer_id)).await?;
    }
    Ok(AutoPullResult { pulled, scripts_held, needs_review, skipped: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FileInfo, ReplaceTarget};

    fn f(path: &str) -> FileInfo {
        FileInfo { relative_path: path.into(), size: 10, hash: "h".into(), modified: 0, file_type: "CustomContent".into() }
    }

    #[test]
    fn only_plain_new_non_script_files_run_unattended() {
        let mut plan = SyncPlan::default();
        plan.actions = vec![
            SyncAction::ReceiveFromRemote(f("Mods/new.package")),
            SyncAction::ReceiveFromRemote(f("Mods/script.ts4script")),
            SyncAction::ReceiveFromRemote(f("BepInEx/plugins/x.DLL")),
            SyncAction::ReceiveFromRemote(f("Mods/custom.myscript.disabled")),
            SyncAction::ReceiveFromRemote(f("Mods/replace.package")),
            SyncAction::ReceiveFromRemote(f("Mods/excluded.package")),
            SyncAction::Conflict { local: f("Mods/c.package"), remote: f("Mods/c.package") },
            SyncAction::Delete("Mods/old.package".into()),
            SyncAction::SendToRemote(f("Mods/mine.package")),
        ];
        plan.use_theirs.insert("Mods/replace.package".into(), ReplaceTarget { local_path: "Mods/replace.package".into(), local_hash: "x".into() });
        plan.excluded.push("Mods/excluded.package".into());
        let (subset, scripts, review) = safe_subset(&plan, &["myscript".into()]);
        let paths: Vec<_> = subset.actions.iter().map(|a| match a { SyncAction::ReceiveFromRemote(f) => f.relative_path.as_str(), _ => "?" }).collect();
        assert_eq!(paths, vec!["Mods/new.package"]);
        assert_eq!(subset.total_bytes, 10);
        assert_eq!(scripts, 3, "ts4script, dll (any case) and the game's own script type, even disabled");
        assert_eq!(review, 3, "the replacement, the conflict and the delete; the excluded file is ignored");
        assert!(subset.plan_hash.is_some());
    }
}
