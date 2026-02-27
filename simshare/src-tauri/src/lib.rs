mod commands;
mod network;
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
                // Detect path outside the lock
                let path = utils::detect_sims4_path();

                if let Some(path) = path {
                    let mods = utils::mods_path(&path);
                    let saves = utils::saves_path(&path);

                    // Start file watcher before acquiring lock
                    let watcher_result = watcher::file_watcher::start_watching(
                        &mods.to_string_lossy(),
                        &saves.to_string_lossy(),
                        handle,
                    );

                    // Acquire lock only to update state
                    let mut app_state = state_clone.lock().await;
                    app_state.sims4_path = Some(path);
                    // Store watcher so it lives as long as AppState
                    if let Ok(w) = watcher_result {
                        app_state.file_watcher = Some(w);
                    }
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
            commands::files::get_sims4_path,
            commands::files::set_sims4_path,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
