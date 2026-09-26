use crate::commands::files::resolve_game;
use crate::state::{AppState, ModProfile, ProfileComparison, ProfileMod};
use crate::utils::{self, sanitize_id};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// file_type values of the game's first (mods) content type.
fn mod_file_types(app_state: &AppState, game: &str) -> Vec<String> {
    crate::commands::files::get_game_def(&app_state.game_registry, game)
        .map(|def| {
            def.content_types
                .first()
                .map(|ct| {
                    let mut types = vec![ct.file_type.clone()];
                    types.extend(ct.classify_by_extension.values().cloned());
                    types
                })
                .unwrap_or_default()
        })
        .unwrap_or_else(|| vec!["Mod".to_string(), "CustomContent".to_string()])
}

#[tauri::command]
pub async fn list_profiles(game: Option<String>) -> Result<Vec<ModProfile>, String> {
    let dir = utils::profiles_dir();
    let mut profiles = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(profile) = serde_json::from_str::<ModProfile>(&data) {
                        profiles.push(profile);
                    }
                }
            }
        }
    }

    // Filter by game if specified (game is now a string ID)
    if let Some(ref game_filter) = game {
        profiles.retain(|p| p.game == *game_filter);
    }

    profiles.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(profiles)
}

#[tauri::command]
pub async fn save_profile(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    name: String,
    desc: String,
    icon: String,
    game: Option<String>,
) -> Result<ModProfile, String> {
    if name.trim().is_empty() || name.len() > 128 {
        return Err("Profile name must be 1-128 characters".to_string());
    }
    if desc.len() > 1024 {
        return Err("Description must be under 1024 characters".to_string());
    }

    // Snapshot the active game only, from a fresh hashed scan: local_manifest
    // may belong to another game or carry empty hashes from a quick scan
    // (every later comparison then reported all mods as "modified").
    let target_game = {
        let app_state = state.lock().await;
        match game {
            Some(ref g) => crate::commands::files::require_active(&app_state, g)?,
            None => app_state.active_game.clone(),
        }
    };
    let manifest = crate::commands::files::scan_files_inner(&state, Some(target_game.clone()), true).await?;
    let app_state = state.lock().await;
    let mod_file_types = mod_file_types(&app_state, &target_game);

    let mods: Vec<ProfileMod> = manifest
        .files
        .values()
        .filter(|f| mod_file_types.contains(&f.file_type))
        .map(|f| ProfileMod {
            relative_path: f.relative_path.clone(),
            hash: f.hash.clone(),
            size: f.size,
            name: std::path::Path::new(&f.relative_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
        })
        .collect();

    let profile = ModProfile {
        id: Uuid::new_v4().to_string(),
        name,
        description: desc,
        icon,
        author: if app_state.local_display_name.is_empty() {
            "Unknown".to_string()
        } else {
            app_state.local_display_name.clone()
        },
        created_at: utils::timestamp_now(),
        mods,
        game: target_game,
    };

    let dir = utils::profiles_dir();
    let path = dir.join(format!("{}.json", profile.id));
    let data = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())?;

    Ok(profile)
}

#[tauri::command]
pub async fn load_profile(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    id: String,
) -> Result<ProfileComparison, String> {
    sanitize_id(&id)?;
    let dir = utils::profiles_dir();
    let path = dir.join(format!("{}.json", id));
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profile: ModProfile = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    {
        let app_state = state.lock().await;
        crate::commands::files::require_active(&app_state, &profile.game)?;
        app_state.active_game_path()?;
    }
    // Hashed: a quick scan's empty hashes made every mod look "modified".
    let manifest = crate::commands::files::scan_files_inner(&state, Some(profile.game.clone()), true).await?;
    let mod_file_types = mod_file_types(&*state.lock().await, &profile.game);

    // Keyed like a sync compare (case- and `.disabled`-insensitive): an exact
    // path match listed a disabled or case-different mod as both "missing"
    // and "extra".
    use crate::sync::diff::match_key;
    let current_mods: std::collections::HashMap<String, (&str, &str)> = manifest
        .files
        .values()
        .filter(|f| mod_file_types.contains(&f.file_type))
        .map(|f| (match_key(&f.relative_path), (f.relative_path.as_str(), f.hash.as_str())))
        .collect();

    let mut missing = Vec::new();
    let mut modified = Vec::new();
    let mut matched = Vec::new();

    for pm in &profile.mods {
        match current_mods.get(&match_key(&pm.relative_path)) {
            None => missing.push(pm.relative_path.clone()),
            Some((_, hash)) if *hash != pm.hash => modified.push(pm.relative_path.clone()),
            _ => matched.push(pm.relative_path.clone()),
        }
    }

    let profile_keys: std::collections::HashSet<String> =
        profile.mods.iter().map(|m| match_key(&m.relative_path)).collect();
    let extra: Vec<String> = current_mods
        .iter()
        .filter(|(k, _)| !profile_keys.contains(k.as_str()))
        .map(|(_, (path, _))| path.to_string())
        .collect();

    Ok(ProfileComparison {
        profile_name: profile.name,
        matched: matched.len(),
        missing,
        modified,
        extra,
    })
}

#[tauri::command]
pub async fn export_profile(id: String, dest: String) -> Result<(), String> {
    sanitize_id(&id)?;
    let dir = utils::profiles_dir();
    let src = dir.join(format!("{}.json", id));
    let data = std::fs::read_to_string(&src).map_err(|e| e.to_string())?;

    let dest_path = if dest.ends_with(".synccrate-profile") || dest.ends_with(".simshare-profile") {
        dest
    } else {
        format!("{}.synccrate-profile", dest)
    };

    std::fs::write(&dest_path, data).map_err(|e| e.to_string())?;
    Ok(())
}

/// Reject anything but plain relative segments: `..`, `\x` / `C:x`
/// (not "absolute" on Windows, yet they escape a join) and `:` (NTFS streams).
fn validate_profile_paths(profile: &ModProfile) -> Result<(), String> {
    for m in &profile.mods {
        let p = std::path::Path::new(&m.relative_path);
        let plain = !m.relative_path.is_empty()
            && !m.relative_path.contains(':')
            && !m.relative_path.starts_with(['/', '\\'])
            && p.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
        if !plain {
            return Err(format!("Invalid mod path in profile: {}", m.relative_path));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn import_profile(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    path: String,
) -> Result<ModProfile, String> {
    // A profile is a file list; anything this big isn't one.
    const MAX_PROFILE_BYTES: u64 = 16 * 1024 * 1024;
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_PROFILE_BYTES {
        return Err("That file is too large to be a SyncCrate profile.".to_string());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut profile: ModProfile = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    sanitize_id(&profile.id)?;
    profile.game = {
        let app_state = state.lock().await;
        resolve_game(&app_state, &profile.game).map_err(|_| format!("Profile is for an unknown game '{}'", profile.game))?
    };
    validate_profile_paths(&profile)?;

    let dir = utils::profiles_dir();
    // Importing a friend's copy of a profile you already have used to
    // overwrite yours silently.
    if dir.join(format!("{}.json", profile.id)).exists() {
        profile.id = Uuid::new_v4().to_string();
    }
    let dest = dir.join(format!("{}.json", profile.id));
    let out = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    std::fs::write(&dest, out).map_err(|e| e.to_string())?;

    Ok(profile)
}

#[tauri::command]
pub async fn delete_profile(id: String) -> Result<(), String> {
    sanitize_id(&id)?;
    let dir = utils::profiles_dir();
    let path = dir.join(format!("{}.json", id));
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(paths: &[&str]) -> ModProfile {
        serde_json::from_value(serde_json::json!({
            "id": "p", "name": "n", "description": "", "icon": "", "author": "a", "created_at": 0,
            "game": "sims4",
            "mods": paths.iter().map(|p| serde_json::json!({"relative_path": p, "hash": "", "size": 0, "name": "x"})).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    #[test]
    fn imported_profile_paths_must_be_plain() {
        assert!(validate_profile_paths(&profile(&["Mods/a.package", "Mods/CC/b.package"])).is_ok());
        for bad in ["../x", "Mods/../../x", r"\Windows\x", "/etc/x", "C:x", r"C:\x", "Mods/a.package:ads", ""] {
            assert!(validate_profile_paths(&profile(&[bad])).is_err(), "{bad}");
        }
    }
}
