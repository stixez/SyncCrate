use crate::state::AppState;
use crate::utils;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

type TagMap = HashMap<String, Vec<String>>;

#[derive(serde::Serialize, serde::Deserialize, Default, Debug)]
struct ModMetadataStore {
    /// Legacy: tags keyed by relative path only, so `Mods/x.package` of one
    /// game showed up on every other game with the same path. Migrated into
    /// `games` on first load and then left empty.
    #[serde(default)]
    tags: TagMap,
    /// game id -> relative path -> tags.
    #[serde(default)]
    games: HashMap<String, TagMap>,
}

fn read_store() -> ModMetadataStore {
    let path = utils::metadata_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&data) {
                return store;
            }
        }
    }
    ModMetadataStore::default()
}

fn write_store(store: &ModMetadataStore) -> Result<(), String> {
    let path = utils::metadata_path();
    let data = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

/// Move legacy path-only tags to `game` (the active game when first seen; the
/// best guess, since before this every game shared one tag map). Returns
/// true if anything changed. Existing per-game tags win on a clash.
fn migrate_legacy(store: &mut ModMetadataStore, game: &str) -> bool {
    if store.tags.is_empty() {
        return false;
    }
    let target = store.games.entry(game.to_string()).or_default();
    for (path, tags) in std::mem::take(&mut store.tags) {
        target.entry(path).or_insert(tags);
    }
    true
}

/// Load the store, migrating legacy tags into `active_game` (persisted once).
fn load_for(active_game: &str) -> ModMetadataStore {
    let mut store = read_store();
    if migrate_legacy(&mut store, active_game) {
        let _ = write_store(&store);
    }
    store
}

/// Re-key tags after `toggle_mod` renamed/moved a file (`x.package` ->
/// `x.package.disabled` or into `_Disabled/`), which used to drop them.
pub(crate) fn move_tags(game: &str, from: &str, to: &str) {
    if from == to {
        return;
    }
    let mut store = load_for(game);
    if move_tags_in(&mut store, game, from, to) {
        let _ = write_store(&store);
    }
}

#[cfg(test)]
pub(crate) fn set_tags_for_test(game: &str, path: &str, tags: &[&str]) {
    let mut store = load_for(game);
    store.games.entry(game.to_string()).or_default().insert(path.to_string(), tags.iter().map(|t| t.to_string()).collect());
    write_store(&store).unwrap();
}

#[cfg(test)]
pub(crate) fn tags_for_test(game: &str, path: &str) -> Vec<String> {
    read_store().games.get(game).and_then(|m| m.get(path)).cloned().unwrap_or_default()
}

fn move_tags_in(store: &mut ModMetadataStore, game: &str, from: &str, to: &str) -> bool {
    let Some(map) = store.games.get_mut(game) else { return false };
    match map.remove(from) {
        Some(tags) => {
            map.insert(to.to_string(), tags);
            true
        }
        None => false,
    }
}

async fn target_game(state: &Arc<Mutex<AppState>>, game_id: &str) -> Result<(String, String), String> {
    let app_state = state.lock().await;
    let id = crate::commands::files::resolve_game(&app_state, game_id)?;
    Ok((id, app_state.active_game.clone()))
}

#[tauri::command]
pub async fn get_predefined_tags() -> Vec<String> {
    vec![
        "CAS".into(),
        "Build".into(),
        "Gameplay".into(),
        "Script".into(),
        "Hair".into(),
        "Clothing".into(),
        "Furniture".into(),
        "Lighting".into(),
        "Terrain".into(),
        "Utility".into(),
        "Fix".into(),
        "Cheat".into(),
    ]
}

#[tauri::command]
pub async fn get_mod_tags(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<TagMap, String> {
    let (game, active) = target_game(&state, &game_id).await?;
    Ok(load_for(&active).games.remove(&game).unwrap_or_default())
}

#[tauri::command]
pub async fn set_mod_tags(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
    path: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let (game, active) = target_game(&state, &game_id).await?;
    let mut store = load_for(&active);
    let map = store.games.entry(game).or_default();
    if tags.is_empty() {
        map.remove(&path);
    } else {
        map.insert(path, tags);
    }
    write_store(&store)
}

#[tauri::command]
pub async fn bulk_set_tags(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
    paths: Vec<String>,
    tags: Vec<String>,
) -> Result<(), String> {
    let (game, active) = target_game(&state, &game_id).await?;
    let mut store = load_for(&active);
    let map = store.games.entry(game).or_default();
    for path in paths {
        if tags.is_empty() {
            map.remove(&path);
        } else {
            let mut existing = map.remove(&path).unwrap_or_default();
            for tag in &tags {
                if !existing.contains(tag) {
                    existing.push(tag.clone());
                }
            }
            map.insert(path, existing);
        }
    }
    write_store(&store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tags_migrate_to_the_active_game_once() {
        let mut store: ModMetadataStore = serde_json::from_str(
            r#"{"tags": {"Mods/a.package": ["CAS"], "Mods/b.package": ["Build"]},
                "games": {"sims4": {"Mods/b.package": ["Hair"]}}}"#,
        )
        .unwrap();
        assert!(migrate_legacy(&mut store, "sims4"));
        assert!(store.tags.is_empty());
        let sims = &store.games["sims4"];
        assert_eq!(sims["Mods/a.package"], vec!["CAS".to_string()]);
        // Newer per-game tags win over the legacy copy.
        assert_eq!(sims["Mods/b.package"], vec!["Hair".to_string()]);
        assert!(!migrate_legacy(&mut store, "sims3"));
        assert!(!store.games.contains_key("sims3"));
    }

    #[test]
    fn old_store_files_still_parse() {
        let store: ModMetadataStore = serde_json::from_str(r#"{"tags": {"x": ["Fix"]}}"#).unwrap();
        assert_eq!(store.tags.len(), 1);
        assert!(store.games.is_empty());
    }

    #[test]
    fn tags_follow_a_toggled_file() {
        let mut store = ModMetadataStore::default();
        store.games.entry("sims4".into()).or_default().insert("Mods/a.package".into(), vec!["CAS".into()]);
        assert!(move_tags_in(&mut store, "sims4", "Mods/a.package", "Mods/a.package.disabled"));
        let sims = &store.games["sims4"];
        assert!(!sims.contains_key("Mods/a.package"));
        assert_eq!(sims["Mods/a.package.disabled"], vec!["CAS".to_string()]);
        assert!(!move_tags_in(&mut store, "sims3", "Mods/a.package", "x"));
    }
}
