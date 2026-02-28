mod commands;
mod network;
mod packs;
mod state;
mod sync;
mod utils;
mod watcher;

use state::{AppState, SimsGame};
use std::sync::Arc;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tokio::sync::Mutex;

pub fn run() {
    env_logger::init();

    let app_state = Arc::new(Mutex::new(AppState::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .setup(|app| {
            // Set up tray icon
            let show = MenuItemBuilder::with_id("show", "Show SimShare").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .tooltip("SimShare")
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            let state: tauri::State<'_, Arc<Mutex<AppState>>> = app.state();
            let state_clone = state.inner().clone();

            tauri::async_runtime::spawn(async move {
                // Load saved config first (persisted paths from previous sessions)
                let saved_config = commands::files::load_game_config();

                // Start with saved paths, then fill gaps with auto-detection
                let mut game_paths = std::collections::HashMap::new();
                for (key, path) in &saved_config.game_paths {
                    if let Ok(game) = commands::files::parse_game(key) {
                        // Only use saved path if it still exists on disk
                        if std::path::Path::new(path).exists() {
                            game_paths.insert(game, path.clone());
                        }
                    }
                }
                // Auto-detect any games not already loaded from config
                for game in &[SimsGame::Sims4, SimsGame::Sims3, SimsGame::Sims2] {
                    if !game_paths.contains_key(game) {
                        if let Some(path) = utils::detect_game_path(game) {
                            game_paths.insert(game.clone(), path);
                        }
                    }
                }

                // Restore saved active game, or pick first detected
                let active_game = saved_config.active_game
                    .and_then(|g| commands::files::parse_game(&g).ok())
                    .filter(|g| game_paths.contains_key(g))
                    .unwrap_or_else(|| {
                        if game_paths.contains_key(&SimsGame::Sims4) {
                            SimsGame::Sims4
                        } else if game_paths.contains_key(&SimsGame::Sims3) {
                            SimsGame::Sims3
                        } else if game_paths.contains_key(&SimsGame::Sims2) {
                            SimsGame::Sims2
                        } else {
                            SimsGame::Sims4
                        }
                    });

                // Start file watcher for active game's paths if available
                let watcher_result = if let Some(path) = game_paths.get(&active_game) {
                    let mods = utils::mods_path(path);
                    let saves = utils::saves_path(path);
                    Some(watcher::file_watcher::start_watching(
                        &mods.to_string_lossy(),
                        &saves.to_string_lossy(),
                        handle,
                    ))
                } else {
                    None
                };

                // Auto-detect packs for all detected games
                let mut game_info_map = std::collections::HashMap::new();
                for (game, path) in &game_paths {
                    let info = packs::detect_game_info(game, path);
                    game_info_map.insert(game.clone(), info);
                }

                // Acquire lock only to update state
                let mut app_state = state_clone.lock().await;
                app_state.game_paths = game_paths;
                app_state.active_game = active_game;
                app_state.game_info = game_info_map;
                if let Some(Ok(w)) = watcher_result {
                    app_state.file_watcher = Some(w);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session::start_host,
            commands::session::start_join,
            commands::session::connect_to_peer,
            commands::session::disconnect,
            commands::session::disconnect_peer,
            commands::session::get_session_status,
            commands::files::scan_files,
            commands::files::get_game_path,
            commands::files::set_game_path,
            commands::files::get_active_game,
            commands::files::set_active_game,
            commands::files::get_all_game_paths,
            commands::sync::compute_sync_plan,
            commands::sync::execute_sync,
            commands::sync::resolve_conflict,
            commands::sync::resolve_all_conflicts,
            commands::profiles::list_profiles,
            commands::profiles::save_profile,
            commands::profiles::load_profile,
            commands::profiles::export_profile,
            commands::profiles::import_profile,
            commands::profiles::delete_profile,
            commands::session::get_app_version,
            commands::session::set_session_port,
            commands::tags::get_predefined_tags,
            commands::tags::get_mod_tags,
            commands::tags::set_mod_tags,
            commands::tags::bulk_set_tags,
            commands::install::install_mod_files,
            commands::install::confirm_install_duplicate,
            commands::backup::create_backup,
            commands::backup::list_backups,
            commands::backup::restore_backup,
            commands::backup::delete_backup,
            commands::sync::update_sync_selection,
            commands::sync::set_exclude_patterns,
            commands::sync::get_exclude_patterns,
            commands::packs::detect_packs,
            commands::packs::get_game_info,
            commands::packs::check_compatibility,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
