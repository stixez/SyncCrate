//! Mod info commands: names, versions, authors and icons read from the
//! metadata files mods ship (`crate::mod_meta`). Read-only.
use crate::mod_meta::{self, IconRef, ModMeta};
use crate::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Icon references from the last metadata pass, so the (large) images are
/// only read when a row actually shows one. Keyed by mod key; cleared when
/// the game folder changes.
static ICONS: std::sync::Mutex<Option<(String, HashMap<String, IconRef>)>> = std::sync::Mutex::new(None);

/// Metadata for the active game's scanned files. Other games return nothing:
/// only the active game has a current manifest.
#[tauri::command]
pub async fn get_mod_metadata(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<Vec<ModMeta>, String> {
    get_mod_metadata_inner(state.inner(), &game).await
}

pub(crate) async fn get_mod_metadata_inner(state: &Arc<Mutex<AppState>>, game: &str) -> Result<Vec<ModMeta>, String> {
    let (base, files) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Ok(Vec::new());
        }
        let base = s.active_game_path()?;
        (base, s.local_manifest.files.keys().cloned().collect::<Vec<_>>())
    };
    let b = base.clone();
    let metas = tokio::task::spawn_blocking(move || mod_meta::extract(&b, &files)).await.map_err(|e| e.to_string())?;
    let icons = metas.iter().filter(|m| m.has_icon).map(|m| (m.key.clone(), m.icon.clone())).collect();
    *ICONS.lock().unwrap_or_else(|e| e.into_inner()) = Some((base, icons));
    Ok(metas)
}

/// The mod's icon as a `data:` URL, or None (not an image, too big, gone).
#[tauri::command]
pub async fn get_mod_icon(key: String) -> Result<Option<String>, String> {
    let found = ICONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|(base, icons)| icons.get(&key).map(|i| (base.clone(), i.clone())));
    let Some((base, icon)) = found else { return Ok(None) };
    tokio::task::spawn_blocking(move || mod_meta::icon_data_url(&base, &key, &icon)).await.map_err(|e| e.to_string())
}
