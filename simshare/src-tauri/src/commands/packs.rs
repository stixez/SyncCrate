use crate::commands::files::parse_game;
use crate::state::{AppState, GameInfo, ModCompatibility};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn detect_packs(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<GameInfo, String> {
    let (target_game, game_path) = {
        let app_state = state.lock().await;
        let tg = match game {
            Some(ref g) => parse_game(g)?,
            None => app_state.active_game.clone(),
        };
        let path = app_state
            .game_paths
            .get(&tg)
            .cloned()
            .ok_or_else(|| format!("Game path not set for {:?}", tg))?;
        (tg, path)
    };

    let tg_clone = target_game.clone();
    let info = tokio::task::spawn_blocking(move || {
        crate::packs::detect_game_info(&tg_clone, &game_path)
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut app_state = state.lock().await;
    app_state.game_info.insert(target_game, info.clone());

    Ok(info)
}

#[tauri::command]
pub async fn get_game_info(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<GameInfo, String> {
    let app_state = state.lock().await;
    let target_game = match game {
        Some(ref g) => parse_game(g)?,
        None => app_state.active_game.clone(),
    };
    Ok(app_state
        .game_info
        .get(&target_game)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub async fn check_compatibility(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<Vec<ModCompatibility>, String> {
    let app_state = state.lock().await;
    let target_game = match game {
        Some(ref g) => parse_game(g)?,
        None => app_state.active_game.clone(),
    };
    let game_info = app_state
        .game_info
        .get(&target_game)
        .cloned()
        .unwrap_or_default();
    let manifest = &app_state.local_manifest;
    Ok(crate::packs::check_mod_compatibility(
        manifest,
        &game_info,
        &target_game,
    ))
}
