use crate::registry::ContentType;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::time::Duration;
use tauri::Emitter;

/// The running watcher. Dropping it stops watching (and ends its thread,
/// which only holds a weak reference).
pub struct FolderWatcher {
    _inner: Arc<Mutex<RecommendedWatcher>>,
}

/// How often folders that didn't exist yet are looked for again.
const MISSING_RECHECK: Duration = Duration::from_secs(5);

/// What to watch for a game.
pub struct WatchSpec {
    /// Content folders, each recursive only when a content type there scans
    /// subfolders: the ReShade presets type (`folder: "."`, not recursive)
    /// had the whole game Bin folder watched with everything below it.
    pub folders: Vec<(String, bool)>,
    /// File extensions a scan can pick up (None: any file can).
    extensions: Option<HashSet<String>>,
    /// File names every content type ignores (ReShade.ini, ...).
    excluded: HashSet<String>,
}

pub fn watch_spec(base: &str, cts: &[ContentType]) -> WatchSpec {
    let mut folders: Vec<(String, bool)> = Vec::new();
    for ct in cts {
        let p = PathBuf::from(base).join(&ct.folder).to_string_lossy().to_string();
        match folders.iter_mut().find(|(f, _)| f.eq_ignore_ascii_case(&p)) {
            Some(entry) => entry.1 |= ct.recursive,
            None => folders.push((p, ct.recursive)),
        }
    }
    let norm = |e: &String| e.trim_start_matches('.').to_lowercase();
    let extensions = (!cts.iter().any(|c| c.extensions.is_empty())).then(|| {
        cts.iter()
            .flat_map(|c| c.extensions.iter().map(norm).chain(c.classify_by_extension.keys().map(norm)))
            // Toggled-off mods (`x.package.disabled`).
            .chain(["disabled".to_string()])
            .collect()
    });
    let excluded = cts.iter().flat_map(|c| c.exclude_files.iter().map(|n| n.to_lowercase())).collect();
    WatchSpec { folders, extensions, excluded }
}

impl WatchSpec {
    /// Whether a change at `path` can change what a scan finds. ReShade and
    /// other tools write logs and settings into watched folders while the
    /// game runs, and each write set off a full rescan. Anything that isn't
    /// a file now (a folder, or a path already gone) always counts.
    fn relevant(&self, path: &Path) -> bool {
        if !path.is_file() {
            return true;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if self.excluded.contains(&name) {
            return false;
        }
        let Some(exts) = &self.extensions else { return true };
        path.extension().is_some_and(|e| exts.contains(&e.to_string_lossy().to_lowercase()))
    }
}

/// Watch `folders`; those that don't exist yet are picked up once they do.
/// Before, a Saves or Mods folder created later (often by the first sync)
/// wasn't watched until the game or its path changed.
fn watch_existing(watcher: &mut RecommendedWatcher, folders: &[(String, bool)]) -> Vec<(String, bool)> {
    let mut missing = Vec::new();
    for (path_str, recursive) in folders {
        let p = Path::new(path_str);
        if !p.is_dir() {
            missing.push((path_str.clone(), *recursive));
            continue;
        }
        let mode = if *recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
        // One unwatchable folder (permissions, network drive, ...) shouldn't
        // disable change detection for all the others.
        if let Err(e) = watcher.watch(p, mode) {
            log::warn!("Cannot watch {}: {}", path_str, e);
        }
    }
    missing
}

/// Start watching a dynamic list of content type directories.
pub fn start_watching(
    spec: WatchSpec,
    app: tauri::AppHandle,
) -> Result<FolderWatcher, String> {
    let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default().with_poll_interval(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let mut missing = watch_existing(&mut watcher, &spec.folders);
    let inner = Arc::new(Mutex::new(watcher));
    let weak = Arc::downgrade(&inner);

    // Spawn a thread to process FS events with debouncing
    let app_handle = app.clone();
    std::thread::spawn(move || {
        // Trailing debounce: emit once the folder has been quiet for `quiet`,
        // or every `max_wait` during a long burst so the list still moves.
        // Emitting on the leading edge of every 500 ms window made a big copy
        // (or a sync writing thousands of files) trigger a full rescan plus a
        // full manifest over IPC twice a second. Nothing is dropped: the
        // final state after a burst always gets one event.
        let quiet = Duration::from_millis(1200);
        let max_wait = Duration::from_secs(5);
        let mut last_event: Option<std::time::Instant> = None;
        let mut first_pending: Option<std::time::Instant> = None;
        let mut pending_paths: Vec<String> = Vec::new();
        let mut pending_kind: Option<String> = None;
        let mut last_recheck = std::time::Instant::now();

        let emit = |paths: &mut Vec<String>, kind: &mut Option<String>| {
            let _ = app_handle.emit(
                "files-changed",
                serde_json::json!({
                    "paths": std::mem::take(paths),
                    "kind": kind.take().unwrap_or_default(),
                }),
            );
        };

        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Ok(event)) => {
                    if !event.paths.iter().any(|p| spec.relevant(p)) {
                        continue;
                    }
                    for p in &event.paths {
                        let s = p.to_string_lossy().to_string();
                        if !pending_paths.contains(&s) && pending_paths.len() < 1000 {
                            pending_paths.push(s);
                        }
                    }
                    pending_kind = Some(format!("{:?}", event.kind));
                    let now = std::time::Instant::now();
                    last_event = Some(now);
                    first_pending.get_or_insert(now);
                }
                Ok(Err(e)) => {
                    log::error!("Watch error: {}", e);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if !missing.is_empty() && last_recheck.elapsed() >= MISSING_RECHECK {
                last_recheck = std::time::Instant::now();
                if missing.iter().any(|(p, _)| Path::new(p).is_dir()) {
                    let Some(w) = weak.upgrade() else { break };
                    let Ok(mut w) = w.lock() else { break };
                    let appeared: Vec<String> = missing.iter().filter(|(p, _)| Path::new(p).is_dir()).map(|(p, _)| p.clone()).collect();
                    missing = watch_existing(&mut w, &missing);
                    // Files may have landed before the watch started: rescan once.
                    pending_paths.extend(appeared);
                    pending_kind.get_or_insert_with(|| "FolderCreated".to_string());
                    let now = std::time::Instant::now();
                    last_event = Some(now);
                    first_pending.get_or_insert(now);
                }
            }

            let settled = last_event.is_some_and(|t| t.elapsed() >= quiet);
            let overdue = first_pending.is_some_and(|t| t.elapsed() >= max_wait);
            if pending_kind.is_some() && (settled || overdue) {
                emit(&mut pending_paths, &mut pending_kind);
                first_pending = None;
            }
        }
    });

    Ok(FolderWatcher { _inner: inner })
}

/// What to watch for the active game (None without a path or definition).
pub fn active_watch_spec(state: &crate::state::AppState) -> Option<WatchSpec> {
    let base = state.game_paths.get(&state.active_game)?;
    let def = state.game_registry.games.iter().find(|g| g.id == state.active_game)?;
    Some(watch_spec(base, &def.content_types)).filter(|s| !s.folders.is_empty())
}

/// (Re)start the watcher for the active game. Previously the watcher was only
/// created at startup, so after switching games (or changing the path) file
/// changes were never picked up. Dropping the old watcher stops its thread.
pub fn restart_for_active(state: &mut crate::state::AppState, app: tauri::AppHandle) {
    state.file_watcher = None;
    let Some(spec) = active_watch_spec(state) else {
        return;
    };
    match start_watching(spec, app) {
        Ok(w) => state.file_watcher = Some(w),
        Err(e) => log::warn!("Failed to start file watcher: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_folders_are_reported_for_a_later_watch() {
        let dir = crate::testutil::temp_dir("watch");
        std::fs::create_dir_all(dir.join("Mods")).unwrap();
        let (tx, _rx) = mpsc::channel::<Result<Event, notify::Error>>();
        let mut w = RecommendedWatcher::new(tx, Config::default()).unwrap();
        let mods = (dir.join("Mods").to_string_lossy().to_string(), true);
        let saves = (dir.join("Saves").to_string_lossy().to_string(), true);
        assert_eq!(watch_existing(&mut w, &[mods.clone(), saves.clone()]), vec![saves.clone()]);
        std::fs::create_dir_all(dir.join("Saves")).unwrap();
        assert!(watch_existing(&mut w, &[saves]).is_empty(), "watched once it exists");
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_changes_a_scan_could_see_count() {
        let reg = crate::registry::load_registry();
        let sims = crate::commands::files::get_game_def(&reg, "sims4-reshade").unwrap();
        let dir = crate::testutil::temp_dir("watch-spec");
        let base = dir.to_string_lossy().to_string();
        let spec = watch_spec(&base, &sims.content_types);
        let bin = spec.folders.iter().find(|(f, _)| f.ends_with('.')).expect("the ReShade Bin folder");
        assert!(!bin.1, "watched without its subfolders");
        assert!(spec.folders.iter().any(|(f, r)| f.ends_with("reshade-shaders") && *r), "{:?}", spec.folders);

        let file = |name: &str| {
            let p = dir.join(name);
            std::fs::write(&p, b"x").unwrap();
            p
        };
        assert!(spec.relevant(&file("Preset.ini")));
        assert!(spec.relevant(&file("Preset.ini.disabled")));
        assert!(!spec.relevant(&file("ReShade.log")), "a log written while playing");
        assert!(!spec.relevant(&file("ReShade.ini")), "an excluded settings file");
        assert!(spec.relevant(&dir.join("gone.ini")), "a deleted file");
        std::fs::create_dir_all(dir.join("v1.2")).unwrap();
        assert!(spec.relevant(&dir.join("v1.2")), "a folder, whatever its name");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
