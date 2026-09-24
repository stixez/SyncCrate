mod commands;
mod game_install;
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
    // Saved paths are kept even when the folder is missing right now (e.g. a
    // game on an unplugged drive): dropping them let auto-detect put a leftover
    // Documents folder in their place, and that got saved over the custom path.
    // The UI flags missing folders instead (`get_unavailable_game_paths`).
    let mut saved = commands::files::SavedPaths::default();
    let mut keys: Vec<&String> = saved_config.game_paths.keys().collect();
    // Current ids before legacy enum names, so a stale legacy entry can't win.
    keys.sort_by_key(|k| !registry_map.contains_key(*k));
    for key in keys {
        if let Some(game_id) = registry::resolve_game_id(key, &registry_map, &legacy_map) {
            if !saved.paths.contains_key(&game_id) {
                if saved_config.user_set_paths.iter().any(|u| u == key || *u == game_id) {
                    saved.user_set.insert(game_id.clone());
                }
                saved.paths.insert(game_id, saved_config.game_paths[key].clone());
            }
        }
    }
    let mut game_paths = saved.paths.clone();
    let user_set: std::collections::HashMap<String, String> = saved
        .paths
        .iter()
        .filter(|(id, _)| saved.user_set.contains(*id))
        .map(|(id, p)| (id.clone(), p.clone()))
        .collect();
    commands::files::set_saved_paths(saved);

    // Auto-detect games without a saved path, but only adopt a folder with
    // install evidence: leftover folders of uninstalled games were adopted
    // (and later got Mods/ Saves/ created in them).
    let install_ctx = game_install::InstallContext::build();
    for game_def in &game_registry.games {
        if game_def.auto_detect && !game_paths.contains_key(&game_def.id) {
            if let Some(path) = utils::detect_game_path_from_def(game_def) {
                if game_install::detect_installed(game_def, &install_ctx, Some(&path), None) {
                    game_paths.insert(game_def.id.clone(), path);
                }
            }
        }
    }

    // A saved folder can still be a leftover (older versions saved detected
    // paths), so first-run picks (active game, default library) prefer games
    // with install evidence.
    let installed: std::collections::HashSet<String> = game_registry
        .games
        .iter()
        .filter(|g| {
            game_paths.get(&g.id).is_some_and(|p| {
                game_install::detect_installed(g, &install_ctx, Some(p.as_str()), user_set.get(&g.id).map(|s| s.as_str()))
            })
        })
        .map(|g| g.id.clone())
        .collect();

    // Restore saved active game, or pick first detected (installed first)
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
                "baldurs_gate_3", "cyberpunk2077", "fallout4", "witcher3",
                "lethal_company", "civilization_6", "stellaris", "hearts_of_iron_4",
                "starfield", "fallout_new_vegas", "crusader_kings_3", "tf2",
                "europa_universalis_4", "farming_simulator_25", "farming_simulator_22",
                "cities_skylines_2", "risk_of_rain_2", "beat_saber", "oblivion",
                "skyrim_le", "xcom2", "american_truck_simulator", "slay_the_spire",
                "hollow_knight", "oxygen_not_included", "victoria_3",
                "mount_blade_warband", "dont_starve", "vintage_story", "morrowind",
                "sims2", "warcraft3", "wow_classic_era",
                "wow_wotlk", "wow_tbc", "wow_vanilla", "wow_custom",
            ];
            for g in &priority {
                if installed.contains(*g) {
                    return g.to_string();
                }
            }
            if let Some(g) = installed.iter().next() {
                return g.clone();
            }
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

    // Restore user library, or build default from installed games with paths
    let user_library = if !saved_config.user_library.is_empty() {
        saved_config.user_library
    } else {
        installed.iter().cloned().collect()
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
        .plugin(tauri_plugin_notification::init())
        .manage(app_state)
        .setup(|app| {
            // Set up tray icon
            let status = MenuItemBuilder::with_id("status", "Idle").enabled(false).build(app)?;
            let show = MenuItemBuilder::with_id("show", "Show SyncCrate").build(app)?;
            let leave = MenuItemBuilder::with_id("leave", "Disconnect").enabled(false).build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&status)
                .separator()
                .item(&show)
                .item(&leave)
                .separator()
                .item(&quit)
                .build()?;

            let tray = TrayIconBuilder::new()
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
                        "leave" => {
                            commands::tray::leave_session_from_tray(app);
                        }
                        "quit" => {
                            commands::tray::QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
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

            app.manage(commands::tray::TrayHandles { tray, status, leave });
            commands::tray::start_tray_status_updates(&handle, state_clone.clone());
            // Scheduled auto-backups (per game, checked every minute)
            commands::backup::spawn_scheduler(handle.clone(), state_clone.clone());

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
        .on_window_event(|window, event| {
            // Close-to-tray: hide the main window instead of quitting when the
            // setting is on. Tray "Quit" sets QUITTING and always exits.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && !commands::tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst)
                    && commands::sync::read_sync_config().close_to_tray
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::session::start_host,
            commands::session::start_join,
            commands::session::connect_to_peer,
            commands::session::connect_by_ip,
            commands::session::connect_by_code,
            commands::session::get_join_code,
            commands::session::disconnect,
            commands::session::disconnect_peer,
            commands::session::get_session_status,
            commands::session::check_host_updates,
            commands::tray::get_close_to_tray,
            commands::tray::set_close_to_tray,
            commands::art::get_game_art,
            commands::art::set_custom_game_art,
            commands::art::clear_custom_game_art,
            commands::art::list_custom_game_art,
            commands::files::scan_files,
            commands::files::get_game_path,
            commands::files::set_game_path,
            commands::files::get_active_game,
            commands::files::set_active_game,
            commands::files::get_all_game_paths,
            commands::files::get_unavailable_game_paths,
            commands::files::toggle_mod,
            commands::files::count_legacy_disabled,
            commands::files::migrate_legacy_disabled,
            commands::files::find_duplicates,
            commands::files::delete_mod_files,
            commands::files::get_game_patch_time,
            commands::files::get_outdated_scripts,
            commands::files::open_folder,
            commands::files::get_game_registry,
            commands::files::get_user_library,
            commands::files::add_to_library,
            commands::files::remove_from_library,
            commands::files::detect_installed_games,
            commands::files::get_installed_games,
            commands::sync::compute_sync_plan,
            commands::sync::execute_sync,
            commands::sync::resolve_conflict,
            commands::sync::resolve_all_conflicts,
            commands::sync::cancel_sync,
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
            commands::sync::get_transfer_speed_limit,
            commands::sync::set_transfer_speed_limit,
            commands::sync::get_sync_history,
            commands::sync::clear_sync_history,
            commands::sync::get_typical_transfer_speed,
            commands::sync::get_clear_cache_after_sync,
            commands::sync::set_clear_cache_after_sync,
            commands::game_state::check_game_running,
            commands::packs::detect_packs,
            commands::packs::get_game_info,
            commands::packs::check_compatibility,
            commands::system::get_firewall_status,
            commands::system::fix_firewall,
            commands::system::is_elevated,
            commands::system::restart_as_admin,
            commands::system::check_game_path_writable,
            commands::system::get_network_diagnostics,
            commands::system::test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
