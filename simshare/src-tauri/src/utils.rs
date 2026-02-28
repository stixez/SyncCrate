use crate::state::SimsGame;
use std::path::PathBuf;

/// Scan all direct children of a directory for a given subfolder path.
/// Handles localized folder names (e.g. OneDrive/Dokumenti, OneDrive/Documenti).
fn scan_children_for(parent: &std::path::Path, sub: &std::path::Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(sub);
            if candidate.exists() {
                results.push(candidate);
            }
        }
    }
    results
}

/// Build candidate paths for a game's EA data folder.
/// `ea_parent` is e.g. "Electronic Arts" or "EA Games".
/// `game_folders` is e.g. &["The Sims 4"].
fn build_candidates(ea_parent: &str, game_folders: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. System Documents folder (handles Windows Known Folders, localized names)
    if let Some(docs) = dirs::document_dir() {
        for folder in game_folders {
            candidates.push(docs.join(ea_parent).join(folder));
        }
    }

    // 2. Windows-specific: scan OneDrive subfolders for localized Documents
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = dirs::home_dir() {
            let onedrive = home.join("OneDrive");
            if onedrive.exists() {
                for folder in game_folders {
                    let sub = PathBuf::from(ea_parent).join(folder);
                    // Scan all children of OneDrive (Documents, Dokumenti, Documenti, etc.)
                    candidates.extend(scan_children_for(&onedrive, &sub));
                }
            }
            // Also check OneDrive - Personal variant
            let onedrive_personal = home.join("OneDrive - Personal");
            if onedrive_personal.exists() {
                for folder in game_folders {
                    let sub = PathBuf::from(ea_parent).join(folder);
                    candidates.extend(scan_children_for(&onedrive_personal, &sub));
                }
            }
        }
    }

    // 3. Linux: common alternate locations
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            for folder in game_folders {
                candidates.push(
                    home.join("Documents").join(ea_parent).join(folder),
                );
            }
        }
    }

    candidates
}

pub fn detect_sims2_path() -> Option<String> {
    let mut candidates = build_candidates("EA Games", &["The Sims 2", "The Sims 2 Ultimate Collection"]);

    // Sims 2 also stores data under "Electronic Arts" on some installs
    candidates.extend(build_candidates("Electronic Arts", &["The Sims 2"]));

    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
}

pub fn detect_sims3_path() -> Option<String> {
    #[allow(unused_mut)]
    let mut candidates = build_candidates("Electronic Arts", &["The Sims 3"]);

    // Linux snap
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("snap")
                    .join("the-sims-3")
                    .join("common")
                    .join("Electronic Arts")
                    .join("The Sims 3"),
            );
        }
    }

    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
}

pub fn detect_sims4_path() -> Option<String> {
    #[allow(unused_mut)]
    let mut candidates = build_candidates("Electronic Arts", &["The Sims 4"]);

    // Linux snap
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("snap")
                    .join("the-sims-4")
                    .join("common")
                    .join("Electronic Arts")
                    .join("The Sims 4"),
            );
        }
    }

    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
}

pub fn detect_game_path(game: &SimsGame) -> Option<String> {
    match game {
        SimsGame::Sims2 => detect_sims2_path(),
        SimsGame::Sims3 => detect_sims3_path(),
        SimsGame::Sims4 => detect_sims4_path(),
    }
}

pub fn game_label(game: &SimsGame) -> &str {
    match game {
        SimsGame::Sims2 => "Sims 2",
        SimsGame::Sims3 => "Sims 3",
        SimsGame::Sims4 => "Sims 4",
    }
}

pub fn valid_mod_extensions(game: &SimsGame) -> &[&str] {
    match game {
        SimsGame::Sims2 => &["package"],
        SimsGame::Sims3 => &["package", "sims3pack", "zip"],
        SimsGame::Sims4 => &["package", "ts4script", "zip", "bpi", "cfg", "txt"],
    }
}

pub fn mods_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Mods")
}

pub fn saves_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Saves")
}

pub fn tray_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Tray")
}

pub fn screenshots_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Screenshots")
}

pub fn profiles_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("simshare").join("profiles");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn timestamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Validate that a relative path does not escape the base directory.
/// Rejects absolute paths, ".." components, and returns a canonical path
/// to prevent TOCTOU symlink attacks.
pub fn safe_join(base: &str, relative: &str) -> Result<PathBuf, String> {
    // Reject absolute paths
    let rel = std::path::Path::new(relative);
    if rel.is_absolute() {
        return Err(format!("Absolute path rejected: {}", relative));
    }

    // Reject paths containing ".." components
    for component in rel.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(format!("Path traversal rejected: {}", relative));
        }
    }

    let joined = PathBuf::from(base).join(relative);

    // Final safety check: canonicalize and verify containment.
    // If the file doesn't exist yet (write case), check parent directory.
    let base_canonical = std::fs::canonicalize(base)
        .map_err(|e| format!("Cannot resolve base path: {}", e))?;

    if joined.exists() {
        let joined_canonical = std::fs::canonicalize(&joined)
            .map_err(|e| format!("Cannot resolve path: {}", e))?;
        if !joined_canonical.starts_with(&base_canonical) {
            return Err(format!("Path escapes base directory: {}", relative));
        }
        // Return canonical path to prevent TOCTOU symlink swaps
        Ok(joined_canonical)
    } else if let Some(parent) = joined.parent() {
        // For new files, resolve through canonical parent to prevent symlink attacks
        if parent.exists() {
            let parent_canonical = std::fs::canonicalize(parent)
                .map_err(|e| format!("Cannot resolve parent path: {}", e))?;
            if !parent_canonical.starts_with(&base_canonical) {
                return Err(format!("Path escapes base directory: {}", relative));
            }
            // Build final path from canonical parent + filename
            let file_name = joined.file_name()
                .ok_or_else(|| format!("Invalid file name in path: {}", relative))?;
            Ok(parent_canonical.join(file_name))
        } else {
            // Parent doesn't exist yet — will be created by caller.
            // Use base_canonical + relative to avoid symlink on base itself.
            Ok(base_canonical.join(relative))
        }
    } else {
        Ok(base_canonical.join(relative))
    }
}

pub fn metadata_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("simshare");
    std::fs::create_dir_all(&dir).ok();
    dir.join("mod_metadata.json")
}

pub fn backups_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("simshare").join("backups");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn sync_config_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("simshare");
    std::fs::create_dir_all(&dir).ok();
    dir.join("sync_config.json")
}

pub fn game_config_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("simshare");
    std::fs::create_dir_all(&dir).ok();
    dir.join("game_config.json")
}

/// Validate a profile ID contains no path separators or traversal
pub fn sanitize_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("ID cannot be empty".to_string());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        return Err("Invalid ID: contains path separators or traversal".to_string());
    }
    Ok(())
}
