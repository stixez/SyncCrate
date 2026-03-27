use crate::registry::{DetectionStrategy, GameDefinition, GameRegistry};
use std::path::PathBuf;

/// Strip the Windows extended-length path prefix (\\?\) that canonicalize() adds.
#[allow(dead_code)]
pub fn clean_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// Scan all direct children of a directory for a given subfolder path.
/// Handles localized folder names (e.g. OneDrive/Dokumenti, OneDrive/Documenti).
#[cfg(target_os = "windows")]
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

/// Build candidate paths for a documents-relative detection strategy.
/// `base` is e.g. "Electronic Arts" or "EA Games".
/// `folders` is e.g. &["The Sims 4"].
fn build_candidates(base: &str, folders: &[String]) -> Vec<PathBuf> {
    let folder_refs: Vec<&str> = folders.iter().map(|s| s.as_str()).collect();
    build_candidates_str(base, &folder_refs)
}

fn build_candidates_str(base: &str, folders: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. System Documents folder (handles Windows Known Folders, localized names)
    if let Some(docs) = dirs::document_dir() {
        for folder in folders {
            candidates.push(docs.join(base).join(folder));
        }
    }

    // 2. Windows-specific: scan OneDrive subfolders for localized Documents
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = dirs::home_dir() {
            let onedrive = home.join("OneDrive");
            if onedrive.exists() {
                for folder in folders {
                    let sub = PathBuf::from(base).join(folder);
                    candidates.extend(scan_children_for(&onedrive, &sub));
                }
            }
            let onedrive_personal = home.join("OneDrive - Personal");
            if onedrive_personal.exists() {
                for folder in folders {
                    let sub = PathBuf::from(base).join(folder);
                    candidates.extend(scan_children_for(&onedrive_personal, &sub));
                }
            }
        }
    }

    // 3. Linux: common alternate locations
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            for folder in folders {
                candidates.push(home.join("Documents").join(base).join(folder));
            }
        }
    }

    candidates
}

/// Detect a game path using the detection strategies from a GameDefinition.
pub fn detect_game_path_from_def(game_def: &GameDefinition) -> Option<String> {
    let detection = game_def.detection.as_ref()?;

    for strategy in &detection.strategies {
        match strategy {
            DetectionStrategy::DocumentsRelative { base, folders } => {
                let candidates = build_candidates(base, folders);
                if let Some(found) = candidates.into_iter().find(|p| p.exists()) {
                    return Some(found.to_string_lossy().to_string());
                }
            }
            DetectionStrategy::AbsolutePaths { paths } => {
                let platform_paths = get_platform_paths(paths);
                for path_str in platform_paths {
                    let expanded = expand_path_vars(path_str);
                    let path = PathBuf::from(&expanded);
                    if path.exists() {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    None
}

/// Expand environment variables and home directory references in paths.
/// Handles `%VAR%` on Windows and `~` on all platforms.
fn expand_path_vars(path: &str) -> String {
    let mut result = path.to_string();

    // Expand ~ to home directory
    if result.starts_with("~/") || result == "~" {
        if let Some(home) = dirs::home_dir() {
            result = result.replacen("~", &home.to_string_lossy(), 1);
        }
    }

    // Expand %VAR% environment variables (Windows-style)
    while let Some(start) = result.find('%') {
        if let Some(end) = result[start + 1..].find('%') {
            let var_name = &result[start + 1..start + 1 + end];
            if let Ok(val) = std::env::var(var_name) {
                result = format!("{}{}{}", &result[..start], val, &result[start + 2 + end..]);
            } else {
                break; // Unknown variable, stop expanding
            }
        } else {
            break; // No closing %, stop
        }
    }

    result
}

/// Get platform-specific paths from PlatformPaths.
fn get_platform_paths(paths: &crate::registry::PlatformPaths) -> &[String] {
    #[cfg(target_os = "windows")]
    {
        &paths.windows
    }
    #[cfg(target_os = "macos")]
    {
        &paths.macos
    }
    #[cfg(target_os = "linux")]
    {
        &paths.linux
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        &[]
    }
}

/// Detect a game path by looking up its definition in the registry.
pub fn detect_game_path_from_registry(game_id: &str, registry: &GameRegistry) -> Option<String> {
    let game_def = registry.games.iter().find(|g| g.id == game_id)?;
    detect_game_path_from_def(game_def)
}

/// Get the list of valid mod/content extensions for a game from its registry definition.
/// Returns extensions from the first content type (primary content).
pub fn valid_extensions_for_game(game_def: &GameDefinition) -> Vec<String> {
    game_def
        .content_types
        .iter()
        .flat_map(|ct| ct.extensions.clone())
        .collect()
}

/// Get the path for a specific content type folder.
#[allow(dead_code)]
pub fn content_type_path(base: &str, folder: &str) -> PathBuf {
    PathBuf::from(base).join(folder)
}

pub fn profiles_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate").join("profiles");
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
    let rel = std::path::Path::new(relative);
    if rel.is_absolute() {
        return Err(format!("Absolute path rejected: {}", relative));
    }

    for component in rel.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(format!("Path traversal rejected: {}", relative));
        }
    }

    let joined = PathBuf::from(base).join(relative);

    let base_canonical = std::fs::canonicalize(base)
        .map_err(|e| format!("Cannot resolve base path: {}", e))?;

    if joined.exists() {
        let joined_canonical = std::fs::canonicalize(&joined)
            .map_err(|e| format!("Cannot resolve path: {}", e))?;
        if !joined_canonical.starts_with(&base_canonical) {
            return Err(format!("Path escapes base directory: {}", relative));
        }
        Ok(joined_canonical)
    } else if let Some(parent) = joined.parent() {
        if parent.exists() {
            let parent_canonical = std::fs::canonicalize(parent)
                .map_err(|e| format!("Cannot resolve parent path: {}", e))?;
            if !parent_canonical.starts_with(&base_canonical) {
                return Err(format!("Path escapes base directory: {}", relative));
            }
            let file_name = joined.file_name()
                .ok_or_else(|| format!("Invalid file name in path: {}", relative))?;
            Ok(parent_canonical.join(file_name))
        } else {
            Ok(base_canonical.join(relative))
        }
    } else {
        Ok(base_canonical.join(relative))
    }
}

pub fn metadata_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("mod_metadata.json")
}

pub fn backups_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate").join("backups");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn sync_config_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("sync_config.json")
}

pub fn game_config_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("game_config.json")
}

pub fn hash_cache_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hash_cache.json")
}

/// Extensions that should never be accepted from a peer during sync.
/// Note: .jar and .dll are intentionally excluded — they are legitimate mod
/// formats for Minecraft Java and Stardew Valley (SMAPI) respectively.
/// Those are handled by the per-game `dangerous_script_extensions` warning system instead.
const DANGEROUS_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "ps1", "vbs", "scr", "lnk",
    "sys", "com", "pif", "msi", "app", "sh", "bash",
    "cpl", "inf", "reg", "ws", "wsf", "hta",
];

/// Returns true if the file extension is on the blocklist of dangerous executables.
pub fn is_dangerous_extension(path: &str) -> bool {
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        DANGEROUS_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Migrate config from the old `simshare` directory to `synccrate`.
/// Safe to call multiple times — does nothing if synccrate dir already exists.
pub fn migrate_from_simshare() {
    let config = match dirs::config_dir() {
        Some(c) => c,
        None => return,
    };
    let new_dir = config.join("synccrate");
    let old_dir = config.join("simshare");

    if new_dir.exists() && std::fs::read_dir(&new_dir).ok().map_or(false, |mut d| d.next().is_some()) {
        migrate_profile_extensions(&new_dir.join("profiles"));
        return;
    }

    if !old_dir.exists() {
        return;
    }

    log::info!("Migrating config from simshare to synccrate");

    if let Err(e) = copy_dir_recursive(&old_dir, &new_dir) {
        log::error!("Config migration failed: {}", e);
        return;
    }

    migrate_profile_extensions(&new_dir.join("profiles"));

    log::info!("Config migration complete");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn migrate_profile_extensions(profiles_dir: &std::path::Path) {
    if !profiles_dir.exists() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("simshare-profile") {
                let new_path = path.with_extension("synccrate-profile");
                if !new_path.exists() {
                    let _ = std::fs::copy(&path, &new_path);
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_join_normal_path() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let result = safe_join(&base_str, "Mods/file.package");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("Mods"));
        assert!(path.to_string_lossy().contains("file.package"));
    }

    #[test]
    fn test_safe_join_blocks_traversal() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let result = safe_join(&base_str, "../../../etc/passwd");
        assert!(result.is_err(), "path traversal should be rejected");
    }

    #[test]
    fn test_safe_join_blocks_absolute_path() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        #[cfg(target_os = "windows")]
        let result = safe_join(&base_str, "C:\\Windows\\System32\\cmd.exe");
        #[cfg(not(target_os = "windows"))]
        let result = safe_join(&base_str, "/etc/passwd");
        assert!(result.is_err(), "absolute paths should be rejected");
    }

    #[test]
    fn test_sanitize_id_valid() {
        let result = sanitize_id("abc-123-def");
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_id_rejects_slashes() {
        assert!(sanitize_id("../malicious").is_err());
        assert!(sanitize_id("path/to/file").is_err());
    }

    #[test]
    fn test_sanitize_id_rejects_dots() {
        assert!(sanitize_id("..").is_err());
    }

    #[test]
    fn test_dangerous_extension_blocks_executables() {
        assert!(is_dangerous_extension("file.exe"));
        assert!(is_dangerous_extension("script.bat"));
        assert!(is_dangerous_extension("script.cmd"));
        assert!(is_dangerous_extension("file.scr"));
    }

    #[test]
    fn test_dangerous_extension_allows_mod_formats() {
        assert!(!is_dangerous_extension("mod.jar"));
        assert!(!is_dangerous_extension("mod.dll"));
        assert!(!is_dangerous_extension("cc.package"));
        assert!(!is_dangerous_extension("config.xml"));
        assert!(!is_dangerous_extension("texture.png"));
    }
}
