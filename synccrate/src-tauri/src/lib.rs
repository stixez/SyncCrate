mod commands;
mod network;
mod packs;
mod registry;
mod state;
mod sync;
mod utils;
mod watcher;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tokio::sync::Mutex;

pub fn run() {
    env_logger::init();

    // Migrate config from old simshare dir if needed (before any config loads)
    utils::migrate_from_simshare();

    // Load game registry (embedded JSON, immutable after init)
    let game_registry = registry::load_registry();
    let registry_map = registry::build_registry_map(&game_registry);
    let legacy_map = registry::build_legacy_map(&game_registry);

    // Load saved config synchronously so paths are available immediately
    let saved_config = commands::files::load_game_config();
    let mut game_paths = std::collections::HashMap::new();
    for (key, path) in &saved_config.game_paths {
        // Accept both new IDs and legacy enum-style names
        if let Some(game_id) = registry::resolve_game_id(key, &registry_map, &legacy_map) {
            if std::path::Path::new(path).exists() {
                game_paths.insert(game_id, path.clone());
            }
        }
    }

    // Auto-detect any games not already loaded from config
    for game_def in &game_registry.games {
        if game_def.auto_detect && !game_paths.contains_key(&game_def.id) {
            if let Some(path) = utils::detect_game_path_from_def(game_def) {
                game_paths.insert(game_def.id.clone(), path);
            }
        }
    }

    // Restore saved active game, or pick first detected
    let active_game = saved_config.active_game
        .and_then(|g| registry::resolve_game_id(&g, &registry_map, &legacy_map))
        .filter(|g| game_paths.contains_key(g))
        .unwrap_or_else(|| {
            // Prefer games roughly by popularity / install likelihood
            let priority = [
                "sims4", "minecraft_java", "wow_retail", "sims3",
                "stardew_valley", "valheim", "terraria", "cod4", "cod2", "cod1",
                "cs2", "gmod", "stronghold_crusader_hd", "stronghold_hd",
                "stronghold_crusader_2", "stronghold_2", "space_engineers",
                "trackmania2020", "tm2_stadium", "tmnf", "tmuf",
                "satisfactory", "dst", "conan_exiles", "torchlight2", "riftbreaker",
                "subnautica", "7daystodie", "kerbal_space_program",
                "wow_classic",
                "sims2", "warcraft3", "wow_classic_era",
                "wow_wotlk", "wow_tbc", "wow_vanilla", "wow_custom",
            ];
            for g in &priority {
                if game_paths.contains_key(*g) {
                    return g.to_string();
                }
            }
            if let Some(g) = game_paths.keys().next() {
                return g.clone();
            }
            "sims4".to_string()
        });

    // Restore user library, or build default from games with configured paths
    let user_library = if !saved_config.user_library.is_empty() {
        saved_config.user_library
    } else {
        game_paths.keys().cloned().collect()
    };

    let mut initial_state = AppState::default();
    initial_state.game_paths = game_paths;
    initial_state.active_game = active_game;
    initial_state.game_registry = game_registry;
    initial_state.user_library = user_library;
    let app_state = Arc::new(Mutex::new(initial_state));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .setup(|app| {
            // Set up tray icon
            let show = MenuItemBuilder::with_id("show", "Show SyncCrate").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .tooltip("SyncCrate")
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
            let state_for_timer = state_clone.clone();

            // Start scheduled auto-backup timer
            {
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let config = crate::commands::sync::read_sync_config();
                        if !config.auto_backup_scheduled {
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                            continue;
                        }

                        let interval = std::time::Duration::from_secs(
                            config.auto_backup_interval_hours as u64 * 3600
                        );
                        tokio::time::sleep(interval).await;

                        let config = crate::commands::sync::read_sync_config();
                        if config.auto_backup_scheduled {
                            log::info!("Creating scheduled auto-backup");
                            if let Err(e) = crate::commands::backup::create_auto_backup(
                                &state_for_timer,
                                &app_handle,
                                "Scheduled",
                            ).await {
                                log::warn!("Scheduled auto-backup failed: {}", e);
                            }
                        }
                    }
                });
            }

            // Async tasks: file watcher + pack detection
            tauri::async_runtime::spawn(async move {
                let app_state = state_clone.lock().await;
                let game_paths = app_state.game_paths.clone();
                let active_game = app_state.active_game.clone();
                let game_registry = app_state.game_registry.clone();
                drop(app_state);

                // Start file watcher for active game's content type folders
                let watcher_result = if let Some(base_path) = game_paths.get(&active_game) {
                    let game_def = game_registry.games.iter().find(|g| g.id == active_game);
                    let watch_paths: Vec<String> = game_def
                        .map(|def| {
                            def.content_types
                                .iter()
                                .map(|ct| {
                                    std::path::PathBuf::from(base_path)
                                        .join(&ct.folder)
                                        .to_string_lossy()
                                        .to_string()
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(watcher::file_watcher::start_watching(&watch_paths, handle))
                } else {
                    None
                };

                // Auto-detect packs for all detected games
                let mut game_info_map = std::collections::HashMap::new();
                for (game_id, path) in &game_paths {
                    let info = packs::detect_game_info(game_id, path);
                    game_info_map.insert(game_id.clone(), info);
                }

                // Acquire lock only to update state
                let mut app_state = state_clone.lock().await;
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
            commands::session::connect_by_ip,
            commands::session::disconnect,
            commands::session::disconnect_peer,
            commands::session::get_session_status,
            commands::files::scan_files,
            commands::files::get_game_path,
            commands::files::set_game_path,
            commands::files::get_active_game,
            commands::files::set_active_game,
            commands::files::get_all_game_paths,
            commands::files::toggle_mod,
            commands::files::open_folder,
            commands::files::get_game_registry,
            commands::files::get_user_library,
            commands::files::add_to_library,
            commands::files::remove_from_library,
            commands::files::detect_installed_games,
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
            commands::session::check_port_available,
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
            commands::backup::rename_backup,
            commands::sync::update_sync_selection,
            commands::sync::set_exclude_patterns,
            commands::sync::get_exclude_patterns,
            commands::sync::get_auto_backup_config,
            commands::sync::set_auto_backup_config,
            commands::packs::detect_packs,
            commands::packs::get_game_info,
            commands::packs::check_compatibility,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
