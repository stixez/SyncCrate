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

/// Automatic tries per host file (same path and hash) before it waits for a
/// manual sync. A file that always fails (locked on the host, a stale hash)
/// was pulled again every minute, forever, up to 4 GB each time.
const MAX_AUTO_ATTEMPTS: u32 = 2;

/// Path -> (host hash, automatic tries so far).
static ATTEMPTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, (String, u32)>>> =
    std::sync::LazyLock::new(Default::default);

/// Take back the try `drop_repeated` counted for `paths` (the pull didn't run).
pub(crate) fn uncount_tries(paths: &[String], attempts: &mut std::collections::HashMap<String, (String, u32)>) {
    for p in paths {
        if let Some(e) = attempts.get_mut(p) {
            e.1 = e.1.saturating_sub(1);
        }
    }
}

/// Drop files already tried `MAX_AUTO_ATTEMPTS` times unchanged, and count
/// this try for the rest. Entries for files no longer planned (pulled, or
/// gone from the host) are forgotten. Returns how many were held back. Pure
/// apart from `attempts`.
pub(crate) fn drop_repeated(subset: &mut SyncPlan, attempts: &mut std::collections::HashMap<String, (String, u32)>) -> usize {
    let planned: std::collections::HashSet<&str> = subset
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncAction::ReceiveFromRemote(f) => Some(f.relative_path.as_str()),
            _ => None,
        })
        .collect();
    attempts.retain(|p, _| planned.contains(p.as_str()));
    let before = subset.actions.len();
    subset.actions.retain(|a| match a {
        SyncAction::ReceiveFromRemote(f) => !attempts.get(&f.relative_path).is_some_and(|(h, n)| h == &f.hash && *n >= MAX_AUTO_ATTEMPTS),
        _ => true,
    });
    let held = before - subset.actions.len();
    for a in &subset.actions {
        if let SyncAction::ReceiveFromRemote(f) = a {
            let e = attempts.entry(f.relative_path.clone()).or_insert_with(|| (f.hash.clone(), 0));
            if e.0 != f.hash {
                *e = (f.hash.clone(), 0);
            }
            e.1 += 1;
        }
    }
    if held > 0 {
        subset.total_bytes = subset
            .actions
            .iter()
            .map(|a| if let SyncAction::ReceiveFromRemote(f) = a { f.size } else { 0 })
            .sum();
        subset.plan_hash = Some(crate::sync::diff::compute_plan_hash(subset));
    }
    held
}

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
    /// New files held because together they're over the unattended size cap.
    pub too_large: usize,
    /// New files that failed to arrive twice in a row (held until a manual sync).
    pub kept_failing: usize,
    /// Why nothing ran, if nothing did (shown in the UI).
    pub skipped: Option<String>,
}

/// The part of a freshly computed plan that's safe to run unattended, plus
/// what was held back. Pure.
/// Whether a file runs code: the always-on list plus the game's
/// `dangerous_script_extensions`.
pub(crate) fn is_script_path(path: &str, script_exts: &[String]) -> bool {
    let ext = crate::commands::files::effective_extension(std::path::Path::new(path));
    SCRIPT_EXTENSIONS.contains(&ext.as_str()) || script_exts.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

/// (subset, scripts held, changes to review, new files held for size)
pub(crate) fn safe_subset(plan: &SyncPlan, script_exts: &[String]) -> (SyncPlan, usize, usize, usize) {
    let is_script = |path: &str| is_script_path(path, script_exts);
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
    let mut too_large = 0;
    if subset.total_bytes > MAX_AUTO_PULL_BYTES {
        too_large = subset.actions.len();
        subset.actions.clear();
        subset.total_bytes = 0;
    }
    subset.plan_hash = Some(crate::sync::diff::compute_plan_hash(&subset));
    (subset, scripts, review, too_large)
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
    // Computed without storing: storing it here replaced a plan the user
    // opened while this ran (up to two minutes), so Sync ran the wrong files.
    let (_, plan) = crate::commands::sync::plan_for_peer(state, Some(peer_id.clone())).await?;
    let (mut subset, scripts_held, needs_review, too_large) = safe_subset(&plan, &script_exts);
    let kept_failing = drop_repeated(&mut subset, &mut ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner()));
    let pulled = subset.actions.len();
    // Counted above as a try; these returns never download anything, and
    // after two of them a healthy file waited for a manual sync as "kept failing".
    let counted: Vec<String> = subset
        .actions
        .iter()
        .filter_map(|a| if let SyncAction::ReceiveFromRemote(f) = a { Some(f.relative_path.clone()) } else { None })
        .collect();
    let uncount = || uncount_tries(&counted, &mut ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner()));
    {
        let mut s = state.lock().await;
        if s.is_any_syncing() {
            uncount();
            return skip("A sync or restore is running.");
        }
        let Some(conn) = s.connections.get_mut(&peer_id) else {
            uncount();
            return skip("Disconnected.");
        };
        if conn.sync_plan.as_ref().is_some_and(|p| !p.actions.is_empty()) {
            uncount();
            return skip("A sync plan is open.");
        }
        if plan.warning.is_some() {
            // e.g. an old host that seems to share another game: never unattended.
            conn.sync_plan = None;
            uncount();
            return Ok(AutoPullResult { needs_review: needs_review + pulled, scripts_held, too_large, kept_failing, skipped: plan.warning.clone(), pulled: 0 });
        }
        conn.sync_plan = (pulled > 0).then_some(subset);
    }
    if pulled > 0 {
        NEEDS_RESCAN.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::commands::sync::execute_sync_inner(state, events, Some(peer_id)).await?;
    }
    Ok(AutoPullResult { pulled, scripts_held, needs_review, too_large, kept_failing, skipped: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_that_never_ran_gives_its_try_back() {
        let mut attempts = std::collections::HashMap::from([("Mods/a.package".to_string(), ("h".to_string(), 2u32))]);
        uncount_tries(&["Mods/a.package".to_string(), "Mods/unknown.package".to_string()], &mut attempts);
        assert_eq!(attempts["Mods/a.package"].1, 1);
        uncount_tries(&["Mods/a.package".to_string()], &mut attempts);
        uncount_tries(&["Mods/a.package".to_string()], &mut attempts);
        assert_eq!(attempts["Mods/a.package"].1, 0, "never below zero");
    }
    use crate::state::{FileInfo, ReplaceTarget};

    fn f(path: &str) -> FileInfo {
        FileInfo { relative_path: path.into(), size: 10, hash: "h".into(), modified: 0, file_type: "CustomContent".into() }
    }

    #[test]
    fn files_that_keep_failing_wait_for_a_manual_sync() {
        let plan = |files: &[(&str, &str)]| {
            let mut p = SyncPlan::default();
            p.actions = files.iter().map(|(path, hash)| SyncAction::ReceiveFromRemote(FileInfo { hash: hash.to_string(), ..f(path) })).collect();
            p
        };
        let mut attempts = std::collections::HashMap::new();
        let mut p = plan(&[("Mods/stuck.package", "a"), ("Mods/ok.package", "b")]);
        assert_eq!(drop_repeated(&mut p, &mut attempts), 0);
        // ok.package arrived; stuck.package is still planned with the same hash.
        let mut p = plan(&[("Mods/stuck.package", "a")]);
        assert_eq!(drop_repeated(&mut p, &mut attempts), 0, "second try");
        assert!(!attempts.contains_key("Mods/ok.package"), "pulled files are forgotten");
        let mut p = plan(&[("Mods/stuck.package", "a")]);
        assert_eq!(drop_repeated(&mut p, &mut attempts), 1, "held back after two tries");
        assert!(p.actions.is_empty() && p.total_bytes == 0);
        // The host changed the file: try again.
        let mut p = plan(&[("Mods/stuck.package", "new")]);
        assert_eq!(drop_repeated(&mut p, &mut attempts), 0);
        assert_eq!(p.actions.len(), 1);
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
        let (subset, scripts, review, too_large) = safe_subset(&plan, &["myscript".into()]);
        assert_eq!(too_large, 0);
        let paths: Vec<_> = subset.actions.iter().map(|a| match a { SyncAction::ReceiveFromRemote(f) => f.relative_path.as_str(), _ => "?" }).collect();
        assert_eq!(paths, vec!["Mods/new.package"]);
        assert_eq!(subset.total_bytes, 10);
        assert_eq!(scripts, 3, "ts4script, dll (any case) and the game's own script type, even disabled");
        assert_eq!(review, 3, "the replacement, the conflict and the delete; the excluded file is ignored");
        assert!(subset.plan_hash.is_some());
    }
}
