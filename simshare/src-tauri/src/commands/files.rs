use crate::state::{AppState, FileInfo, FileManifest, FileType, SimsGame};
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
        let _ = std::fs::write(&path, data);
    }
}

pub(crate) fn parse_game(game: &str) -> Result<SimsGame, String> {
    match game {
        "Sims2" => Ok(SimsGame::Sims2),
        "Sims3" => Ok(SimsGame::Sims3),
        "Sims4" => Ok(SimsGame::Sims4),
        _ => Err(format!("Unknown game: {}", game)),
    }
}

fn game_to_string(game: &SimsGame) -> String {
    match game {
        SimsGame::Sims2 => "Sims2".to_string(),
        SimsGame::Sims3 => "Sims3".to_string(),
        SimsGame::Sims4 => "Sims4".to_string(),
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct GameConfig {
    pub game_paths: HashMap<String, String>,
    pub active_game: Option<String>,
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

fn save_game_config(app_state: &AppState) {
    let mut game_paths = HashMap::new();
    for (game, path) in &app_state.game_paths {
        game_paths.insert(game_to_string(game), path.clone());
    }
    let config = GameConfig {
        game_paths,
        active_game: Some(game_to_string(&app_state.active_game)),
    };
    let path = utils::game_config_path();
    if let Ok(data) = serde_json::to_string_pretty(&config) {
        let _ = std::fs::write(&path, data);
    }
}

fn scan_directory(
    base_path: &str,
    sub_dir: &str,
    file_type_fn: impl Fn(&str) -> FileType + Sync,
    valid_extensions: &[&str],
    compute_hashes: bool,
    hash_cache: &HashCache,
) -> HashMap<String, FileInfo> {
    let dir = std::path::PathBuf::from(base_path).join(sub_dir);
    let mut files = HashMap::new();

    if !dir.exists() {
        return files;
    }

    // Collect eligible file entries first, then hash in parallel
    let entries: Vec<_> = WalkDir::new(&dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            if entry.path_is_symlink() || !entry.path().is_file() {
                return false;
            }
            if sub_dir == "Mods" {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                valid_extensions.contains(&ext.as_str())
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
                // Check cache: if size + mtime match, reuse cached hash
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
    let mut reader = std::io::BufReader::with_capacity(131072, file); // 128 KB buffer
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 131072]; // 128 KB read chunks
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[tauri::command]
pub async fn scan_files(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
    quick: Option<bool>,
) -> Result<FileManifest, String> {
    let compute_hashes = !quick.unwrap_or(false);
    let (base_path, active_game) = {
        let mut app_state = state.lock().await;
        let target_game = match game {
            Some(ref g) => parse_game(g)?,
            None => app_state.active_game.clone(),
        };
        let path = app_state
            .game_paths
            .get(&target_game)
            .cloned()
            .or_else(|| {
                let detected = utils::detect_game_path(&target_game);
                if let Some(ref p) = detected {
                    app_state.game_paths.insert(target_game.clone(), p.clone());
                }
                detected
            })
            .ok_or_else(|| {
                format!(
                    "{} path not found. Please set it manually.",
                    utils::game_label(&target_game)
                )
            })?;
        (path, target_game)
    };

    let extensions: Vec<String> = utils::valid_mod_extensions(&active_game)
        .iter()
        .map(|s| s.to_string())
        .collect();

    let manifest = tokio::task::spawn_blocking(move || {
        let hash_cache = if compute_hashes { load_hash_cache() } else { HashMap::new() };
        let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
        let mods = scan_directory(&base_path, "Mods", |ext| match ext {
            "ts4script" | "zip" | "sims3pack" => FileType::Mod,
            _ => FileType::CustomContent,
        }, &ext_refs, compute_hashes, &hash_cache);

        let saves = scan_directory(&base_path, "Saves", |_| FileType::Save, &[], compute_hashes, &hash_cache);
        let tray = scan_directory(&base_path, "Tray", |_| FileType::Tray, &[], compute_hashes, &hash_cache);
        let screenshots = scan_directory(&base_path, "Screenshots", |_| FileType::Screenshot, &[], compute_hashes, &hash_cache);

        let mut all_files = mods;
        all_files.extend(saves);
        all_files.extend(tray);
        all_files.extend(screenshots);

        // Update hash cache with current scan results
        if compute_hashes {
            let mut new_cache = HashCache::new();
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

    let mut app_state = state.lock().await;
    app_state.local_manifest = manifest.clone();
    Ok(manifest)
}

#[tauri::command]
pub async fn get_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
) -> Result<String, String> {
    let target_game = parse_game(&game)?;
    let app_state = state.lock().await;
    app_state
        .game_paths
        .get(&target_game)
        .cloned()
        .or_else(|| utils::detect_game_path(&target_game))
        .ok_or_else(|| format!("{} path not found", utils::game_label(&target_game)))
}

#[tauri::command]
pub async fn set_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
    path: String,
) -> Result<(), String> {
    let target_game = parse_game(&game)?;
    let game_dir = std::path::Path::new(&path);
    if !game_dir.exists() {
        return Err("Path does not exist".to_string());
    }
    // Canonicalize to resolve symlinks and normalize the path
    let mut canonical = utils::clean_path(
        std::fs::canonicalize(game_dir)
            .map_err(|e| format!("Cannot resolve path: {}", e))?,
    );
    // Auto-correct if user selected a known subfolder instead of the game root
    if let Some(folder_name) = canonical.file_name().and_then(|n| n.to_str()) {
        let known_subfolders = ["Mods", "Saves", "Tray", "Screenshots", "Downloads"];
        if known_subfolders.iter().any(|&s| s.eq_ignore_ascii_case(folder_name)) {
            if let Some(parent) = canonical.parent() {
                canonical = parent.to_path_buf();
            }
        }
    }
    // Validate this looks like a Sims folder — or accept it and create Mods/Saves
    let has_mods = canonical.join("Mods").exists();
    let has_saves = canonical.join("Saves").exists();
    // Sims 2 uses "Downloads" instead of "Mods" sometimes
    let has_downloads = canonical.join("Downloads").exists();
    if !has_mods && !has_saves && !has_downloads {
        // If the folder exists but has no Mods/Saves, create them so the app can work
        std::fs::create_dir_all(canonical.join("Mods"))
            .map_err(|e| format!("Cannot create Mods folder: {}", e))?;
        std::fs::create_dir_all(canonical.join("Saves"))
            .map_err(|e| format!("Cannot create Saves folder: {}", e))?;
    }
    let mut app_state = state.lock().await;
    app_state.game_paths.insert(target_game, canonical.to_string_lossy().to_string());
    save_game_config(&app_state);
    Ok(())
}

#[tauri::command]
pub async fn get_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let app_state = state.lock().await;
    Ok(game_to_string(&app_state.active_game))
}

#[tauri::command]
pub async fn set_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
) -> Result<(), String> {
    let target_game = parse_game(&game)?;
    let mut app_state = state.lock().await;
    app_state.active_game = target_game;
    save_game_config(&app_state);
    Ok(())
}

#[tauri::command]
pub async fn toggle_mod(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    relative_path: String,
    enabled: bool,
) -> Result<String, String> {
    let base = {
        let app_state = state.lock().await;
        app_state
            .game_paths
            .get(&app_state.active_game)
            .cloned()
            .ok_or("Game path not set")?
    };

    let full_path = utils::safe_join(&base, &relative_path)?;
    if !full_path.exists() {
        return Err("File not found".into());
    }

    let mods_dir = utils::mods_path(&base);
    let disabled_dir = mods_dir.join("_Disabled");

    let filename = full_path
        .file_name()
        .ok_or("Invalid filename")?
        .to_os_string();

    if enabled {
        // Move from _Disabled back to Mods root
        let dest = mods_dir.join(&filename);
        if dest.exists() {
            return Err("A file with that name already exists in Mods".into());
        }
        std::fs::rename(&full_path, &dest).map_err(|e| e.to_string())?;
        let new_rel = dest
            .strip_prefix(&base)
            .unwrap_or(&dest)
            .to_string_lossy()
            .replace('\\', "/");
        Ok(new_rel)
    } else {
        // Move to _Disabled
        std::fs::create_dir_all(&disabled_dir).map_err(|e| e.to_string())?;
        let dest = disabled_dir.join(&filename);
        if dest.exists() {
            return Err("A file with that name already exists in _Disabled".into());
        }
        std::fs::rename(&full_path, &dest).map_err(|e| e.to_string())?;
        let new_rel = dest
            .strip_prefix(&base)
            .unwrap_or(&dest)
            .to_string_lossy()
            .replace('\\', "/");
        Ok(new_rel)
    }
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
    for game in &[SimsGame::Sims2, SimsGame::Sims3, SimsGame::Sims4] {
        result.insert(game_to_string(game), app_state.game_paths.get(game).cloned());
    }
    Ok(result)
}
