use crate::commands::files::{get_game_def, resolve_game};
use crate::state::AppState;
use crate::utils;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_INSTALL_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024; // 2 GB

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub source: String,
    pub destination: String,
    pub status: InstallStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InstallStatus {
    Success,
    Duplicate,
    InvalidExtension,
    Failed,
}

fn file_hash(path: &Path) -> Result<String, String> {
    // Stream instead of reading the whole file (up to 2 GB) into memory.
    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot read file: {}", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("Cannot read file: {}", e))?;
    Ok(hex::encode(hasher.finalize()))
}

/// "name_N.ext", or "name_N" for extension-less files (no trailing dot, which
/// Windows silently strips).
fn numbered_name(stem: &str, counter: u32, ext: &str) -> String {
    if ext.is_empty() {
        format!("{}_{}", stem, counter)
    } else {
        format!("{}_{}.{}", stem, counter, ext)
    }
}

#[tauri::command]
pub async fn install_mod_files(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    file_paths: Vec<String>,
    game: Option<String>,
) -> Result<Vec<InstallResult>, String> {
    let (base, game_id) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        // Install into the *target* game's folder. active_game_path() put files
        // for a non-active `game` into the active game's directory.
        let path = app_state.game_paths.get(&game_id).cloned().ok_or_else(|| {
            format!("{} path not set. Please set it first.", app_state.game_label(&game_id))
        })?;
        (path, game_id)
    };

    // Get valid extensions and install folder from game definition
    let (valid_extensions, install_folder, game_label) = {
        let app_state = state.lock().await;
        let game_def = get_game_def(&app_state.game_registry, &game_id);
        let exts: Vec<String> = game_def
            .map(|def| utils::valid_extensions_for_game(def))
            .unwrap_or_default();
        let folder = game_def
            .and_then(|def| def.content_types.first())
            .map(|ct| ct.folder.clone())
            .unwrap_or_else(|| "Mods".to_string());
        let label = app_state.game_label(&game_id);
        (exts, folder, label)
    };

    let mods_dir = std::path::PathBuf::from(&base).join(&install_folder);
    if !mods_dir.exists() {
        std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    }

    let mut results = Vec::new();

    for source_str in &file_paths {
        let source = Path::new(source_str);

        if !source.is_file() {
            results.push(InstallResult {
                source: source_str.clone(),
                destination: String::new(),
                status: InstallStatus::Failed,
                message: Some("File does not exist".into()),
            });
            continue;
        }

        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !valid_extensions.iter().any(|e| e == &ext) {
            let supported: Vec<String> = valid_extensions.iter().map(|e| format!(".{}", e)).collect();
            results.push(InstallResult {
                source: source_str.clone(),
                destination: String::new(),
                status: InstallStatus::InvalidExtension,
                message: Some(format!(
                    "Invalid extension '.{}'. Supported for {}: {}",
                    ext,
                    game_label,
                    supported.join(", ")
                )),
            });
            continue;
        }

        let metadata = std::fs::metadata(source).map_err(|e| e.to_string())?;
        if metadata.len() > MAX_INSTALL_FILE_SIZE {
            results.push(InstallResult {
                source: source_str.clone(),
                destination: String::new(),
                status: InstallStatus::Failed,
                message: Some("File exceeds 2 GB size limit".into()),
            });
            continue;
        }

        let file_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") || file_name.contains('\0') {
            results.push(InstallResult {
                source: source_str.clone(),
                destination: String::new(),
                status: InstallStatus::Failed,
                message: Some("Invalid filename".into()),
            });
            continue;
        }

        let dest = mods_dir.join(file_name);

        if dest.exists() {
            let source_hash = match file_hash(source) {
                Ok(h) => h,
                Err(e) => {
                    results.push(InstallResult {
                        source: source_str.clone(),
                        destination: dest.to_string_lossy().to_string(),
                        status: InstallStatus::Failed,
                        message: Some(e),
                    });
                    continue;
                }
            };
            let dest_hash = match file_hash(&dest) {
                Ok(h) => h,
                Err(e) => {
                    results.push(InstallResult {
                        source: source_str.clone(),
                        destination: dest.to_string_lossy().to_string(),
                        status: InstallStatus::Failed,
                        message: Some(e),
                    });
                    continue;
                }
            };

            if source_hash == dest_hash {
                results.push(InstallResult {
                    source: source_str.clone(),
                    destination: dest.to_string_lossy().to_string(),
                    status: InstallStatus::Duplicate,
                    message: Some("Identical file already exists".into()),
                });
            } else {
                results.push(InstallResult {
                    source: source_str.clone(),
                    destination: dest.to_string_lossy().to_string(),
                    status: InstallStatus::Duplicate,
                    message: Some("File with same name but different content exists".into()),
                });
            }
            continue;
        }

        match std::fs::copy(source, &dest) {
            Ok(_) => {
                results.push(InstallResult {
                    source: source_str.clone(),
                    destination: dest.to_string_lossy().to_string(),
                    status: InstallStatus::Success,
                    message: None,
                });
            }
            Err(e) => {
                results.push(InstallResult {
                    source: source_str.clone(),
                    destination: dest.to_string_lossy().to_string(),
                    status: InstallStatus::Failed,
                    message: Some(format!("Copy failed: {}", e)),
                });
            }
        }
    }

    Ok(results)
}

#[tauri::command]
pub async fn confirm_install_duplicate(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    source: String,
    strategy: String,
    game: Option<String>,
) -> Result<InstallResult, String> {
    let (base, game_id) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        // Install into the *target* game's folder. active_game_path() put files
        // for a non-active `game` into the active game's directory.
        let path = app_state.game_paths.get(&game_id).cloned().ok_or_else(|| {
            format!("{} path not set. Please set it first.", app_state.game_label(&game_id))
        })?;
        (path, game_id)
    };

    let install_folder = {
        let app_state = state.lock().await;
        get_game_def(&app_state.game_registry, &game_id)
            .and_then(|def| def.content_types.first())
            .map(|ct| ct.folder.clone())
            .unwrap_or_else(|| "Mods".to_string())
    };

    let mods_dir = std::path::PathBuf::from(&base).join(&install_folder);
    let source_path = Path::new(&source);

    if !source_path.is_file() {
        return Err("Source file no longer exists".into());
    }

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;

    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") || file_name.contains('\0') {
        return Err("Invalid filename".into());
    }

    match strategy.as_str() {
        "overwrite" => {
            let dest = mods_dir.join(file_name);
            // Copying a file onto itself (dropping a file that already lives in
            // the mods folder) can truncate it; treat it as already installed.
            if let (Ok(a), Ok(b)) = (std::fs::canonicalize(source_path), std::fs::canonicalize(&dest)) {
                if a == b {
                    return Ok(InstallResult {
                        source,
                        destination: dest.to_string_lossy().to_string(),
                        status: InstallStatus::Success,
                        message: Some("File is already in place".into()),
                    });
                }
            }
            std::fs::copy(source_path, &dest).map_err(|e| e.to_string())?;
            Ok(InstallResult {
                source,
                destination: dest.to_string_lossy().to_string(),
                status: InstallStatus::Success,
                message: Some("Overwritten".into()),
            })
        }
        "rename" => {
            let stem = source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            let ext = source_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let mut counter = 1u32;
            let mut dest = mods_dir.join(numbered_name(stem, counter, ext));
            while dest.exists() {
                counter += 1;
                if counter > 999 {
                    return Err("Too many duplicates".into());
                }
                dest = mods_dir.join(numbered_name(stem, counter, ext));
            }

            std::fs::copy(source_path, &dest).map_err(|e| e.to_string())?;
            Ok(InstallResult {
                source,
                destination: dest.to_string_lossy().to_string(),
                status: InstallStatus::Success,
                message: Some(format!("Renamed to {}", dest.file_name().unwrap().to_string_lossy())),
            })
        }
        _ => Err(format!("Unknown strategy: {}", strategy)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_name_without_extension_has_no_trailing_dot() {
        assert_eq!(numbered_name("mod", 2, "package"), "mod_2.package");
        assert_eq!(numbered_name("README", 1, ""), "README_1");
    }

    #[test]
    fn file_hash_streams_correctly() {
        let p = std::env::temp_dir().join(format!("synccrate_hash_{}.bin", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            file_hash(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_file(&p);
    }
}
