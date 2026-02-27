use std::path::PathBuf;

pub fn detect_sims4_path() -> Option<String> {
    let mut candidates = Vec::new();

    // Standard: Documents/Electronic Arts/The Sims 4 (works on all platforms via dirs)
    if let Some(docs) = dirs::document_dir() {
        candidates.push(docs.join("Electronic Arts").join("The Sims 4"));
    }

    // Windows: also check OneDrive-redirected Documents
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("OneDrive")
                    .join("Documents")
                    .join("Electronic Arts")
                    .join("The Sims 4"),
            );
        }
    }

    // Linux: common alternate locations
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("Documents")
                    .join("Electronic Arts")
                    .join("The Sims 4"),
            );
            // Snap installation
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

pub fn mods_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Mods")
}

pub fn saves_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("Saves")
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
