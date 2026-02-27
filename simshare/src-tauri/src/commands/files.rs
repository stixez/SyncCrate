use crate::state::{AppState, FileInfo, FileManifest, FileType, SimsGame};
use crate::utils;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use walkdir::WalkDir;

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

fn scan_directory(
    base_path: &str,
    sub_dir: &str,
    file_type_fn: impl Fn(&str) -> FileType,
    valid_extensions: &[&str],
) -> HashMap<String, FileInfo> {
    let dir = std::path::PathBuf::from(base_path).join(sub_dir);
    let mut files = HashMap::new();

    if !dir.exists() {
        return files;
    }

    for entry in WalkDir::new(&dir).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        // Skip symlinks to prevent escaping base directory
        if entry.path_is_symlink() {
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if sub_dir == "Mods" && !valid_extensions.contains(&ext.as_str()) {
            continue;
        }

        let relative = path
            .strip_prefix(base_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
            .replace('\\', "/"); // Normalize to forward slashes for cross-platform compat

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let hash = match compute_file_hash(path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_type = file_type_fn(&ext);

        files.insert(
            relative.clone(),
            FileInfo {
                relative_path: relative,
                size: metadata.len(),
                hash,
                modified,
                file_type,
            },
        );
    }

    files
}

fn compute_file_hash(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
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
) -> Result<FileManifest, String> {
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
        let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
        let mods = scan_directory(&base_path, "Mods", |ext| match ext {
            "ts4script" | "zip" | "sims3pack" => FileType::Mod,
            _ => FileType::CustomContent,
        }, &ext_refs);

        let saves = scan_directory(&base_path, "Saves", |_| FileType::Save, &[]);
        let tray = scan_directory(&base_path, "Tray", |_| FileType::Tray, &[]);
        let screenshots = scan_directory(&base_path, "Screenshots", |_| FileType::Screenshot, &[]);

        let mut all_files = mods;
        all_files.extend(saves);
        all_files.extend(tray);
        all_files.extend(screenshots);

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
    let canonical = std::fs::canonicalize(game_dir)
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    // Validate this looks like a Sims folder
    let has_mods = canonical.join("Mods").exists();
    let has_saves = canonical.join("Saves").exists();
    // Sims 2 uses "Downloads" instead of "Mods" sometimes
    let has_downloads = canonical.join("Downloads").exists();
    if !has_mods && !has_saves && !has_downloads {
        return Err(format!(
            "This doesn't look like a {} folder (no Mods or Saves directory found)",
            utils::game_label(&target_game)
        ));
    }
    let mut app_state = state.lock().await;
    app_state.game_paths.insert(target_game, canonical.to_string_lossy().to_string());
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
