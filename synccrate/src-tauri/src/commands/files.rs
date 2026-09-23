use crate::registry::{self, GameDefinition};
use crate::state::{AppState, FileInfo, FileManifest};
use crate::utils;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use walkdir::WalkDir;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct HashCacheEntry {
    size: u64,
    mtime: u64,
    hash: String,
}

type HashCache = HashMap<String, HashCacheEntry>;

fn load_hash_cache() -> HashCache {
    let path = utils::hash_cache_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(cache) = serde_json::from_str(&data) {
                return cache;
            }
        }
    }
    HashMap::new()
}

fn save_hash_cache(cache: &HashCache) {
    let path = utils::hash_cache_path();
    if let Ok(data) = serde_json::to_string(cache) {
        // Atomic write via a unique temp file + rename so a concurrent scan (e.g. the
        // host warm-up scan racing an on-demand rehash) can't leave a half-written
        // cache. The unique suffix avoids two writers clobbering the same temp file.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = path.with_extension(format!("{}.tmp", unique));
        if std::fs::write(&tmp, &data).is_ok() {
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct GameConfig {
    pub game_paths: HashMap<String, String>,
    pub active_game: Option<String>,
    #[serde(default)]
    pub user_library: Vec<String>,
}

pub fn load_game_config() -> GameConfig {
    let path = utils::game_config_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<GameConfig>(&data) {
                return config;
            }
        }
    }
    GameConfig::default()
}

pub(crate) fn save_game_config(app_state: &AppState) {
    let config = GameConfig {
        game_paths: app_state.game_paths.clone(),
        active_game: Some(app_state.active_game.clone()),
        user_library: app_state.user_library.clone(),
    };
    let path = utils::game_config_path();
    if let Ok(data) = serde_json::to_string_pretty(&config) {
        let _ = std::fs::write(&path, data);
    }
}

/// Resolve a game ID string, accepting both new registry IDs and legacy enum variant names.
pub(crate) fn resolve_game(app_state: &AppState, game_str: &str) -> Result<String, String> {
    let registry_map = registry::build_registry_map(&app_state.game_registry);
    let legacy_map = registry::build_legacy_map(&app_state.game_registry);
    registry::resolve_game_id(game_str, &registry_map, &legacy_map)
        .ok_or_else(|| format!("Unknown game: {}", game_str))
}

/// Get the GameDefinition for a game ID.
pub(crate) fn get_game_def<'a>(registry: &'a crate::registry::GameRegistry, game_id: &str) -> Option<&'a GameDefinition> {
    registry.games.iter().find(|g| g.id == game_id)
}

fn scan_directory(
    base_path: &str,
    sub_dir: &str,
    file_type_fn: impl Fn(&str) -> String + Sync,
    valid_extensions: &[String],
    compute_hashes: bool,
    hash_cache: &HashCache,
) -> HashMap<String, FileInfo> {
    let dir = std::path::PathBuf::from(base_path).join(sub_dir);
    let mut files = HashMap::new();

    if !dir.exists() {
        return files;
    }

    let ext_refs: Vec<&str> = valid_extensions.iter().map(|s| s.as_str()).collect();

    // Collect eligible file entries first, then hash in parallel
    let entries: Vec<_> = WalkDir::new(&dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            if entry.path_is_symlink() || !entry.path().is_file() {
                return false;
            }
            // If extensions list is non-empty, filter by them
            if !ext_refs.is_empty() {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                ext_refs.contains(&ext.as_str())
            } else {
                true
            }
        })
        .collect();

    let results: Vec<_> = entries
        .par_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let relative = path
                .strip_prefix(base_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");

            let metadata = std::fs::metadata(path).ok()?;
            let file_size = metadata.len();

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let hash = if compute_hashes {
                let cache_key = path.to_string_lossy().replace('\\', "/");
                if let Some(cached) = hash_cache.get(&cache_key) {
                    if cached.size == file_size && cached.mtime == modified {
                        cached.hash.clone()
                    } else {
                        compute_file_hash(path).ok()?
                    }
                } else {
                    compute_file_hash(path).ok()?
                }
            } else {
                String::new()
            };

            let file_type = file_type_fn(&ext);

            Some((
                relative.clone(),
                FileInfo {
                    relative_path: relative,
                    size: file_size,
                    hash,
                    modified,
                    file_type,
                },
            ))
        })
        .collect();

    for (key, info) in results {
        files.insert(key, info);
    }

    files
}

fn compute_file_hash(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::with_capacity(131072, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 131072];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Scan files for the active game, optionally computing hashes.
/// This is the shared implementation used by both the Tauri command and internal callers.
pub async fn scan_files_inner(
    state: &Arc<Mutex<AppState>>,
    game: Option<String>,
    compute_hashes: bool,
) -> Result<FileManifest, String> {
    let (game_id, base_path, game_def, newly_detected) = {
        let mut app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let (path, newly_detected) = match app_state.game_paths.get(&game_id).cloned() {
            Some(p) => (p, false),
            None => {
                let detected = utils::detect_game_path_from_registry(&game_id, &app_state.game_registry)
                    .ok_or_else(|| {
                        format!(
                            "{} path not found. Please set it manually.",
                            app_state.game_label(&game_id)
                        )
                    })?;
                app_state.game_paths.insert(game_id.clone(), detected.clone());
                (detected, true)
            }
        };
        let def = get_game_def(&app_state.game_registry, &game_id)
            .ok_or_else(|| format!("Game '{}' not found in registry", game_id))?
            .clone();
        (game_id, path, def, newly_detected)
    };
    let scanned_base = base_path.clone();

    // A just-auto-detected path bypasses set_game_path, which is where the game's
    // expected content folders normally get created. Mirror that here so, e.g.,
    // ReShade's reshade-presets/ folder exists on first detection rather than only
    // when the user manually re-picks the path.
    if newly_detected {
        if let Some(val) = &game_def.validation {
            for dir in &val.auto_create_dirs {
                let _ = std::fs::create_dir_all(std::path::Path::new(&base_path).join(dir));
            }
        }
    }

    let manifest = tokio::task::spawn_blocking(move || {
        let hash_cache = if compute_hashes { load_hash_cache() } else { HashMap::new() };

        let mut all_files = HashMap::new();

        // Data-driven scanning: iterate content types from registry
        for ct in &game_def.content_types {
            let classify = ct.classify_by_extension.clone();
            let default_ft = ct.file_type.clone();
            let exts = ct.extensions.clone();

            let files = scan_directory(
                &base_path,
                &ct.folder,
                move |ext| {
                    classify.get(ext).cloned().unwrap_or_else(|| default_ft.clone())
                },
                &exts,
                compute_hashes,
                &hash_cache,
            );
            all_files.extend(files);
        }

        // Update hash cache with current scan results. Keep entries that belong
        // to other games/folders — replacing the whole cache with just this
        // scan threw away every other game's cached hashes on each game switch.
        if compute_hashes {
            let base_prefix = format!(
                "{}/",
                base_path.replace('\\', "/").trim_end_matches('/')
            );
            let mut new_cache: HashCache = hash_cache
                .into_iter()
                .filter(|(k, _)| !k.starts_with(&base_prefix))
                .collect();
            for info in all_files.values() {
                if !info.hash.is_empty() {
                    let abs_path = std::path::PathBuf::from(&base_path)
                        .join(&info.relative_path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    new_cache.insert(abs_path, HashCacheEntry {
                        size: info.size,
                        mtime: info.modified,
                        hash: info.hash.clone(),
                    });
                }
            }
            save_hash_cache(&new_cache);
        }

        FileManifest {
            files: all_files,
            generated_at: utils::timestamp_now(),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    // `local_manifest` is what we sync against and serve to peers, and it is
    // always interpreted relative to the *active* game's path. Only store the
    // result if this scan was for the active game (and its path didn't change
    // mid-scan); scanning another game (e.g. browsing it in the library) must
    // not replace the active game's manifest.
    let mut manifest = manifest;
    let mut app_state = state.lock().await;
    if app_state.active_game == game_id
        && app_state.game_paths.get(&game_id) == Some(&scanned_base)
    {
        if !compute_hashes {
            // A quick scan (e.g. triggered by the file watcher) has no hashes.
            // Carry over known hashes for unchanged files so a quick rescan
            // doesn't wipe the hashes a sync plan / peer manifest relies on
            // (empty local hashes make every shared file look like a conflict).
            carry_over_hashes(&mut manifest, &app_state.local_manifest);
        }
        app_state.local_manifest = manifest.clone();
    }
    Ok(manifest)
}

/// Fill empty hashes in `new` from `old` for files whose size and mtime are unchanged.
fn carry_over_hashes(new: &mut FileManifest, old: &FileManifest) {
    for (path, info) in new.files.iter_mut() {
        if !info.hash.is_empty() {
            continue;
        }
        if let Some(prev) = old.files.get(path) {
            if !prev.hash.is_empty() && prev.size == info.size && prev.modified == info.modified {
                info.hash = prev.hash.clone();
            }
        }
    }
}

#[tauri::command]
pub async fn scan_files(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
    quick: Option<bool>,
) -> Result<FileManifest, String> {
    let compute_hashes = !quick.unwrap_or(false);
    scan_files_inner(&*state, game, compute_hashes).await
}

#[tauri::command]
pub async fn get_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
) -> Result<String, String> {
    let app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    app_state
        .game_paths
        .get(&game_id)
        .cloned()
        .or_else(|| utils::detect_game_path_from_registry(&game_id, &app_state.game_registry))
        .ok_or_else(|| format!("{} path not found", app_state.game_label(&game_id)))
}

#[tauri::command]
pub async fn set_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    game: String,
    path: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    let game_def = get_game_def(&app_state.game_registry, &game_id)
        .ok_or_else(|| format!("Game '{}' not found in registry", game_id))?
        .clone();

    let game_dir = std::path::Path::new(&path);
    if !game_dir.exists() {
        return Err("Path does not exist".to_string());
    }

    let mut canonical = utils::clean_path(
        std::fs::canonicalize(game_dir)
            .map_err(|e| format!("Cannot resolve path: {}", e))?,
    );

    // Auto-correct if user selected a known subfolder
    if let Some(pc) = &game_def.path_correction {
        if let Some(folder_name) = canonical.file_name().and_then(|n| n.to_str()) {
            let folder_str = folder_name.to_string();

            // Check nested corrections first (e.g., AddOns -> go up 2 levels)
            if let Some(&levels) = pc.nested_corrections.get(&folder_str) {
                for _ in 0..levels {
                    if let Some(parent) = canonical.parent() {
                        canonical = parent.to_path_buf();
                    }
                }
            } else if pc.known_subfolders.iter().any(|s| s.eq_ignore_ascii_case(&folder_str)) {
                if let Some(parent) = canonical.parent() {
                    canonical = parent.to_path_buf();
                }
            }
        }
    }

    // Validate or create expected directories
    if let Some(val) = &game_def.validation {
        let any_exists = val.check_dirs.is_empty()
            || val.check_dirs.iter().any(|d| canonical.join(d).exists());

        if !any_exists && !val.auto_create_dirs.is_empty() {
            for dir in &val.auto_create_dirs {
                std::fs::create_dir_all(canonical.join(dir))
                    .map_err(|e| format!("Cannot create {} folder: {}", dir, e))?;
            }
        }
    }

    let new_path = canonical.to_string_lossy().to_string();
    if app_state.active_game == game_id
        && app_state.game_paths.get(&game_id) != Some(&new_path)
    {
        // Manifest paths were relative to the old folder.
        app_state.local_manifest = FileManifest::default();
    }
    let is_active = app_state.active_game == game_id;
    app_state.game_paths.insert(game_id, new_path);
    save_game_config(&app_state);
    if is_active {
        crate::watcher::file_watcher::restart_for_active(&mut app_state, app);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let app_state = state.lock().await;
    Ok(app_state.active_game.clone())
}

#[tauri::command]
pub async fn set_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    game: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    let changed = app_state.active_game != game_id;
    if changed && app_state.session_type != crate::state::SessionType::None {
        // Peers are syncing against this game's manifest/folder.
        return Err("Disconnect from the current session before switching games.".to_string());
    }
    if changed {
        // The manifest belongs to the previous game; don't keep serving/syncing
        // it against the new game's path until a rescan replaces it.
        app_state.local_manifest = FileManifest::default();
    }
    app_state.active_game = game_id;
    save_game_config(&app_state);
    if changed {
        crate::watcher::file_watcher::restart_for_active(&mut app_state, app);
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_mod(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    relative_path: String,
    enabled: bool,
) -> Result<String, String> {
    let (base, first_content_folder) = {
        let app_state = state.lock().await;
        let base = app_state
            .game_paths
            .get(&app_state.active_game)
            .cloned()
            .ok_or("Game path not set")?;
        let folder = get_game_def(&app_state.game_registry, &app_state.active_game)
            .and_then(|d| d.content_types.first())
            .map(|ct| ct.folder.clone())
            .unwrap_or_else(|| "Mods".to_string());
        (base, folder)
    };

    let full_path = utils::safe_join(&base, &relative_path)?;
    if !full_path.exists() {
        return Err("File not found".into());
    }

    let mods_dir = std::path::PathBuf::from(&base).join(&first_content_folder);
    let dest = match toggle_destination(&mods_dir, &full_path, enabled)? {
        Some(d) => d,
        None => {
            // Already in the requested state; nothing to move.
            return Ok(relative_path.replace('\\', "/"));
        }
    };

    if dest.exists() {
        return Err(if enabled {
            format!("A file with that name already exists in {}", first_content_folder)
        } else {
            "A file with that name already exists in _Disabled".to_string()
        });
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&full_path, &dest).map_err(|e| e.to_string())?;

    let base_canonical = utils::clean_path(std::fs::canonicalize(&base).map_err(|e| e.to_string())?);
    let dest_clean = utils::clean_path(dest.clone());
    let new_rel = dest_clean
        .strip_prefix(&base_canonical)
        .unwrap_or(&dest_clean)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(new_rel)
}

/// Work out where `toggle_mod` should move a file.
///
/// Keeps the file's subfolder structure under the mods folder:
/// `Mods/Creator/x.package` <-> `Mods/_Disabled/Creator/x.package`.
/// (The old code flattened everything into `_Disabled/` and restored it to the
/// mods root, losing the original folder and colliding same-named files.)
///
/// Returns `Some(dest)`, or `None` when the file is already
/// in the requested state. Errors if the file isn't inside the mods folder.
fn toggle_destination(
    mods_dir: &std::path::Path,
    full_path: &std::path::Path,
    enabled: bool,
) -> Result<Option<std::path::PathBuf>, String> {
    let mods_canonical = utils::clean_path(
        std::fs::canonicalize(mods_dir).map_err(|e| format!("Mods folder not found: {}", e))?,
    );
    let file_clean = utils::clean_path(full_path.to_path_buf());
    let rel_in_mods = file_clean
        .strip_prefix(&mods_canonical)
        .map_err(|_| "Only files in the mods folder can be enabled or disabled".to_string())?;

    let mut comps = rel_in_mods.components();
    let in_disabled = matches!(
        comps.next(),
        Some(std::path::Component::Normal(first)) if first.eq_ignore_ascii_case("_Disabled")
    );
    let rest: std::path::PathBuf = comps.collect();

    Ok(match (enabled, in_disabled) {
        (true, true) => Some(mods_canonical.join(&rest)),
        (false, false) => Some(mods_canonical.join("_Disabled").join(rel_in_mods)),
        _ => None,
    })
}

#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("Path does not exist".into());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_game_paths(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<HashMap<String, Option<String>>, String> {
    let app_state = state.lock().await;
    let mut result = HashMap::new();
    for game in &app_state.game_registry.games {
        result.insert(game.id.clone(), app_state.game_paths.get(&game.id).cloned());
    }
    Ok(result)
}

// --- New commands for game registry and library ---

#[tauri::command]
pub async fn get_game_registry(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<registry::GameDefinition>, String> {
    let app_state = state.lock().await;
    Ok(app_state.game_registry.games.clone())
}

#[tauri::command]
pub async fn get_user_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    let app_state = state.lock().await;
    Ok(app_state.user_library.clone())
}

#[tauri::command]
pub async fn add_to_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    if !app_state.game_registry.games.iter().any(|g| g.id == game_id) {
        return Err(format!("Unknown game: {}", game_id));
    }
    if !app_state.user_library.contains(&game_id) {
        app_state.user_library.push(game_id);
        save_game_config(&app_state);
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_from_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    app_state.user_library.retain(|id| id != &game_id);
    save_game_config(&app_state);
    Ok(())
}

#[tauri::command]
pub async fn detect_installed_games(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<HashMap<String, String>, String> {
    let app_state = state.lock().await;
    let registry = app_state.game_registry.clone();
    drop(app_state);

    let result = tokio::task::spawn_blocking(move || {
        let mut detected = HashMap::new();
        for game in &registry.games {
            if game.auto_detect {
                if let Some(path) = utils::detect_game_path_from_def(game) {
                    detected.insert(game.id.clone(), path);
                }
            }
        }
        detected
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str, hash: &str, size: u64, modified: u64) -> FileInfo {
        FileInfo {
            relative_path: path.to_string(),
            size,
            hash: hash.to_string(),
            modified,
            file_type: "Mod".to_string(),
        }
    }

    fn manifest(files: Vec<FileInfo>) -> FileManifest {
        FileManifest {
            files: files.into_iter().map(|f| (f.relative_path.clone(), f)).collect(),
            generated_at: 0,
        }
    }

    #[test]
    fn quick_scan_keeps_hashes_of_unchanged_files_only() {
        let old = manifest(vec![
            info("Mods/same.package", "h1", 10, 100),
            info("Mods/changed.package", "h2", 10, 100),
        ]);
        let mut new = manifest(vec![
            info("Mods/same.package", "", 10, 100),
            info("Mods/changed.package", "", 11, 200),
            info("Mods/new.package", "", 5, 300),
        ]);
        carry_over_hashes(&mut new, &old);
        assert_eq!(new.files["Mods/same.package"].hash, "h1");
        assert_eq!(new.files["Mods/changed.package"].hash, "");
        assert_eq!(new.files["Mods/new.package"].hash, "");
    }

    fn temp_mods_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "synccrate_toggle_{}_{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Mods").join("Creator")).unwrap();
        dir
    }

    #[test]
    fn toggle_preserves_subfolders() {
        let base = temp_mods_dir("subfolders");
        let mods = base.join("Mods");
        let file = mods.join("Creator").join("hair.package");
        std::fs::write(&file, b"x").unwrap();

        let disabled = toggle_destination(&mods, &std::fs::canonicalize(&file).unwrap(), false)
            .unwrap()
            .expect("should move");
        assert!(disabled.ends_with("Mods/_Disabled/Creator/hair.package"));

        std::fs::create_dir_all(disabled.parent().unwrap()).unwrap();
        std::fs::rename(&file, &disabled).unwrap();
        let enabled = toggle_destination(&mods, &std::fs::canonicalize(&disabled).unwrap(), true)
            .unwrap()
            .expect("should move");
        assert!(enabled.ends_with("Mods/Creator/hair.package"));

        // Already disabled -> no move
        assert!(toggle_destination(&mods, &std::fs::canonicalize(&disabled).unwrap(), false)
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn toggle_rejects_files_outside_mods_folder() {
        let base = temp_mods_dir("outside");
        std::fs::create_dir_all(base.join("Saves")).unwrap();
        let save = base.join("Saves").join("slot.save");
        std::fs::write(&save, b"x").unwrap();
        let res = toggle_destination(&base.join("Mods"), &std::fs::canonicalize(&save).unwrap(), false);
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
