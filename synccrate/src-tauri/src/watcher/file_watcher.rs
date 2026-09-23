use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use tauri::Emitter;

/// Start watching a dynamic list of content type directories.
pub fn start_watching(
    paths: &[String],
    app: tauri::AppHandle,
) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default().with_poll_interval(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    for path_str in paths {
        let p = Path::new(path_str);
        if p.exists() {
            // One unwatchable folder (permissions, network drive, ...) shouldn't
            // disable change detection for all the others.
            if let Err(e) = watcher.watch(p, RecursiveMode::Recursive) {
                log::warn!("Cannot watch {}: {}", path_str, e);
            }
        }
    }

    // Spawn a thread to process FS events with debouncing
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let debounce = Duration::from_millis(500);
        // Events are coalesced: the first event of a burst is emitted right away
        // (if we haven't emitted recently); anything arriving inside the debounce
        // window is held and emitted once the window closes. Previously such
        // events were simply dropped, so the final state after a burst (e.g. a
        // large copy finishing) never reached the frontend.
        let mut last_emit: Option<std::time::Instant> = None;
        let mut pending_paths: Vec<String> = Vec::new();
        let mut pending_kind: Option<String> = None;

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
                    for p in &event.paths {
                        let s = p.to_string_lossy().to_string();
                        if !pending_paths.contains(&s) && pending_paths.len() < 1000 {
                            pending_paths.push(s);
                        }
                    }
                    pending_kind = Some(format!("{:?}", event.kind));
                }
                Ok(Err(e)) => {
                    log::error!("Watch error: {}", e);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if pending_kind.is_some()
                && last_emit.map_or(true, |t| t.elapsed() >= debounce)
            {
                emit(&mut pending_paths, &mut pending_kind);
                last_emit = Some(std::time::Instant::now());
            }
        }
    });

    Ok(watcher)
}

/// Content folders of the active game, as absolute paths.
pub fn active_watch_paths(state: &crate::state::AppState) -> Vec<String> {
    let Some(base) = state.game_paths.get(&state.active_game) else {
        return Vec::new();
    };
    state
        .game_registry
        .games
        .iter()
        .find(|g| g.id == state.active_game)
        .map(|def| {
            def.content_types
                .iter()
                .map(|ct| std::path::PathBuf::from(base).join(&ct.folder).to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// (Re)start the watcher for the active game. Previously the watcher was only
/// created at startup, so after switching games (or changing the path) file
/// changes were never picked up. Dropping the old watcher stops its thread.
pub fn restart_for_active(state: &mut crate::state::AppState, app: tauri::AppHandle) {
    state.file_watcher = None;
    let paths = active_watch_paths(state);
    if paths.is_empty() {
        return;
    }
    match start_watching(&paths, app) {
        Ok(w) => state.file_watcher = Some(w),
        Err(e) => log::warn!("Failed to start file watcher: {}", e),
    }
}
