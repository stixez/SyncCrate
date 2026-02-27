use crate::utils;
use std::collections::HashMap;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ModMetadataStore {
    tags: HashMap<String, Vec<String>>,
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
pub async fn get_mod_tags() -> Result<HashMap<String, Vec<String>>, String> {
    Ok(read_store().tags)
}

#[tauri::command]
pub async fn set_mod_tags(path: String, tags: Vec<String>) -> Result<(), String> {
    let mut store = read_store();
    if tags.is_empty() {
        store.tags.remove(&path);
    } else {
        store.tags.insert(path, tags);
    }
    write_store(&store)
}

#[tauri::command]
pub async fn bulk_set_tags(paths: Vec<String>, tags: Vec<String>) -> Result<(), String> {
    let mut store = read_store();
    for path in paths {
        if tags.is_empty() {
            store.tags.remove(&path);
        } else {
            let mut existing = store.tags.remove(&path).unwrap_or_default();
            for tag in &tags {
                if !existing.contains(tag) {
                    existing.push(tag.clone());
                }
            }
            store.tags.insert(path, existing);
        }
    }
    write_store(&store)
}
