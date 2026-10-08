//! Host side of "Share as a link" (`crate::source_links`): which mods this
//! PC shares as a link to their creator instead of copying them.
use crate::source_links::{self, SourceLink};
use crate::state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

async fn known_game(state: &Arc<Mutex<AppState>>, game: &str) -> Result<(), String> {
    let s = state.lock().await;
    if s.game_registry.games.iter().any(|g| g.id == game) {
        Ok(())
    } else {
        Err(format!("Unknown game '{game}'"))
    }
}

#[tauri::command]
pub async fn get_source_links(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<Vec<SourceLink>, String> {
    known_game(state.inner(), &game).await?;
    source_links::load(&game)
}

/// Insert, or replace the link with the same prefix. Returns the game's links.
#[tauri::command]
pub async fn set_source_link(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
    prefix: String,
    url: String,
    label: Option<String>,
) -> Result<Vec<SourceLink>, String> {
    known_game(state.inner(), &game).await?;
    source_links::set_link(&game, &prefix, &url, label.as_deref())
}

#[tauri::command]
pub async fn remove_source_link(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
    prefix: String,
) -> Result<Vec<SourceLink>, String> {
    known_game(state.inner(), &game).await?;
    source_links::remove_link(&game, &prefix)
}
