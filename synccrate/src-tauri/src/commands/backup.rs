use crate::commands::files::{get_game_def, resolve_game};
use crate::state::AppState;
use crate::utils;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::registry::ContentType;
use walkdir::WalkDir;

const MAX_BACKUP_FILES: usize = 100_000;
const MAX_BACKUP_LABEL_LEN: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: u64,
    pub label: String,
    pub file_count: usize,
    pub total_size: u64,
    pub mods_count: usize,
    pub saves_count: usize,
    #[serde(default)]
    pub tray_count: usize,
    #[serde(default)]
    pub screenshots_count: usize,
    #[serde(default = "default_game")]
    pub game: String,
    #[serde(default)]
    pub auto: bool,
}

fn default_game() -> String {
    "sims4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    pub info: BackupInfo,
    pub files: Vec<BackupFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupFileEntry {
    pub relative_path: String,
    pub size: u64,
    pub category: String,
}

fn copy_dir_to_backup(
    source_dir: &Path,
    ct: Option<&ContentType>,
    backup_dir: &Path,
    category: &str,
    app: &tauri::AppHandle,
    files_done: &mut usize,
    files_total: usize,
) -> Result<Vec<BackupFileEntry>, String> {
    let mut entries = Vec::new();

    if !source_dir.exists() {
        return Ok(entries);
    }

    for entry in content_walker(source_dir, ct)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        // Only back up what the content type actually covers — e.g. a
        // non-recursive "." type for loose presets must not copy the whole
        // game folder.
        if let Some(ct) = ct {
            if !crate::commands::files::content_type_accepts(ct, entry.path()) {
                continue;
            }
        }

        if entry.path_is_symlink() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(source_dir)
            .map_err(|e| e.to_string())?;

        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }

        let dest_dir = backup_dir.join(category);
        let dest = dest_dir.join(rel);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        std::fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;

        entries.push(BackupFileEntry {
            relative_path: rel.to_string_lossy().to_string(),
            size: metadata.len(),
            category: category.to_string(),
        });

        *files_done += 1;
        let _ = app.emit(
            "backup-progress",
            serde_json::json!({
                "file": rel.to_string_lossy(),
                "files_done": *files_done,
                "files_total": files_total,
            }),
        );

        if entries.len() > MAX_BACKUP_FILES {
            return Err(format!("Too many files (>{MAX_BACKUP_FILES}). Aborting backup."));
        }
    }

    Ok(entries)
}

/// True if `rel` consists only of normal path segments. `Path::is_absolute`
/// alone isn't enough on Windows: "\foo" and "C:foo" are not absolute but
/// `join` with them escapes the destination folder.
fn is_plain_relative_path(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty()
        && p.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

fn content_walker(dir: &Path, ct: Option<&ContentType>) -> WalkDir {
    let walker = WalkDir::new(dir).follow_links(false);
    match ct {
        Some(ct) if !ct.recursive => walker.max_depth(1),
        _ => walker,
    }
}

fn count_files(dir: &Path, ct: Option<&ContentType>) -> usize {
    if !dir.exists() {
        return 0;
    }
    content_walker(dir, ct)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && !e.path_is_symlink())
        .filter(|e| ct.map_or(true, |ct| crate::commands::files::content_type_accepts(ct, e.path())))
        .count()
}

#[tauri::command]
pub async fn create_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    label: String,
    game: Option<String>,
) -> Result<BackupInfo, String> {
    let label = label.trim().to_string();
    if label.is_empty() || label.len() > MAX_BACKUP_LABEL_LEN {
        return Err(format!("Label must be 1-{MAX_BACKUP_LABEL_LEN} characters"));
    }
    if label.chars().any(|c| c.is_control()) {
        return Err("Label contains invalid characters".into());
    }

    let (base, game_id, content_types) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let path = app_state.game_paths.get(&game_id).cloned()
            .ok_or_else(|| format!("{} path not set", app_state.game_label(&game_id)))?;
        let cts = get_game_def(&app_state.game_registry, &game_id)
            .map(|def| def.content_types.clone())
            .unwrap_or_default();
        (path, game_id, cts)
    };

    // Build directory list from content types
    let mut category_dirs: Vec<(String, std::path::PathBuf, ContentType)> = Vec::new();
    for ct in &content_types {
        category_dirs.push((ct.id.clone(), std::path::PathBuf::from(&base).join(&ct.folder), ct.clone()));
    }

    let files_total: usize = category_dirs.iter().map(|(_, dir, ct)| count_files(dir, Some(ct))).sum();

    let id = Uuid::new_v4().to_string();
    let backup_dir = utils::backups_dir().join(&id);
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    let mut files_done = 0usize;
    let mut all_entries = Vec::new();
    let mut category_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (category, dir, ct) in &category_dirs {
        let entries = match copy_dir_to_backup(dir, Some(ct), &backup_dir, category, &app, &mut files_done, files_total) {
            Ok(e) => e,
            Err(e) => {
                // Don't leave a partial, manifest-less backup folder behind
                // (it never shows up in list_backups, so it can't be deleted).
                let _ = std::fs::remove_dir_all(&backup_dir);
                return Err(e);
            }
        };
        category_counts.insert(category.clone(), entries.len());
        all_entries.extend(entries);
    }

    let total_size: u64 = all_entries.iter().map(|e| e.size).sum();

    let info = BackupInfo {
        id: id.clone(),
        created_at: utils::timestamp_now(),
        label,
        file_count: all_entries.len(),
        total_size,
        mods_count: *category_counts.get("mods").unwrap_or(&0),
        saves_count: *category_counts.get("saves").unwrap_or(&0),
        tray_count: *category_counts.get("tray").unwrap_or(&0),
        screenshots_count: *category_counts.get("screenshots").unwrap_or(&0),
        game: game_id,
        auto: false,
    };

    let manifest = BackupManifest {
        info: info.clone(),
        files: all_entries,
    };

    let manifest_path = backup_dir.join("manifest.json");
    let data = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, data).map_err(|e| e.to_string())?;

    Ok(info)
}

#[tauri::command]
pub async fn list_backups() -> Result<Vec<BackupInfo>, String> {
    let dir = utils::backups_dir();
    let mut backups = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let manifest_path = entry.path().join("manifest.json");
            if manifest_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_json::from_str::<BackupManifest>(&data) {
                        backups.push(manifest.info);
                    }
                }
            }
        }
    }

    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

/// Delete oldest auto-backups for a game until count <= max_count.
pub fn prune_auto_backups(game: &str, max_count: usize) -> Result<(), String> {
    let dir = utils::backups_dir();
    let mut auto_backups: Vec<BackupInfo> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let manifest_path = entry.path().join("manifest.json");
            if manifest_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_json::from_str::<BackupManifest>(&data) {
                        if manifest.info.auto && manifest.info.game == game {
                            auto_backups.push(manifest.info);
                        }
                    }
                }
            }
        }
    }

    auto_backups.sort_by_key(|b| b.created_at);

    while auto_backups.len() > max_count {
        if let Some(oldest) = auto_backups.first() {
            let backup_path = dir.join(&oldest.id);
            if backup_path.exists() {
                let _ = std::fs::remove_dir_all(&backup_path);
            }
            auto_backups.remove(0);
        }
    }

    Ok(())
}

/// Create an auto-backup (called internally by sync or timer, not a Tauri command).
pub async fn create_auto_backup(
    state: &Arc<Mutex<AppState>>,
    app: &tauri::AppHandle,
    label_prefix: &str,
) -> Result<(), String> {
    let (base, game_id, content_types) = {
        let app_state = state.lock().await;
        let game_id = app_state.active_game.clone();
        let path = app_state.game_paths.get(&game_id).cloned()
            .ok_or_else(|| "Game path not set".to_string())?;
        let cts = crate::commands::files::get_game_def(&app_state.game_registry, &game_id)
            .map(|def| def.content_types.clone())
            .unwrap_or_default();
        (path, game_id, cts)
    };

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let label = format!("Auto: {} {}", label_prefix, now);

    let mut category_dirs: Vec<(String, std::path::PathBuf, ContentType)> = Vec::new();
    for ct in &content_types {
        category_dirs.push((ct.id.clone(), std::path::PathBuf::from(&base).join(&ct.folder), ct.clone()));
    }

    let files_total: usize = category_dirs.iter().map(|(_, dir, ct)| count_files(dir, Some(ct))).sum();

    let id = Uuid::new_v4().to_string();
    let backup_dir = utils::backups_dir().join(&id);
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    let mut files_done = 0usize;
    let mut all_entries = Vec::new();
    let mut category_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (category, dir, ct) in &category_dirs {
        let entries = match copy_dir_to_backup(dir, Some(ct), &backup_dir, category, app, &mut files_done, files_total) {
            Ok(e) => e,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                return Err(e);
            }
        };
        category_counts.insert(category.clone(), entries.len());
        all_entries.extend(entries);
    }

    let total_size: u64 = all_entries.iter().map(|e| e.size).sum();

    let info = BackupInfo {
        id: id.clone(),
        created_at: utils::timestamp_now(),
        label,
        file_count: all_entries.len(),
        total_size,
        mods_count: *category_counts.get("mods").unwrap_or(&0),
        saves_count: *category_counts.get("saves").unwrap_or(&0),
        tray_count: *category_counts.get("tray").unwrap_or(&0),
        screenshots_count: *category_counts.get("screenshots").unwrap_or(&0),
        game: game_id.clone(),
        auto: true,
    };

    let manifest = BackupManifest {
        info,
        files: all_entries,
    };

    let manifest_path = backup_dir.join("manifest.json");
    let data = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, data).map_err(|e| e.to_string())?;

    // Prune old auto-backups
    let config = crate::commands::sync::read_sync_config();
    prune_auto_backups(&game_id, config.auto_backup_max_count as usize)?;

    Ok(())
}

#[tauri::command]
pub async fn restore_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    id: String,
) -> Result<RestoreResult, String> {
    utils::sanitize_id(&id)?;

    let backup_dir = utils::backups_dir().join(&id);
    let manifest_path = backup_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err("Backup not found".into());
    }

    let data = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: BackupManifest = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    // Resolve game from backup manifest
    let (base, game_id, content_types) = {
        let app_state = state.lock().await;
        // Restoring rewrites files a sync may be reading/writing, and a host's
        // peers would pull a half-restored folder.
        if app_state.is_any_syncing() {
            return Err("Can't restore while a sync is in progress.".into());
        }
        if app_state.session_type != crate::state::SessionType::None {
            return Err("Disconnect from the current session before restoring a backup.".into());
        }
        // Never fall back to another game: restoring e.g. a Minecraft backup
        // into the Sims 4 folder would scatter files where they don't belong.
        let game_id = resolve_game(&app_state, &manifest.info.game)
            .map_err(|_| format!("Backup is for an unknown game '{}'", manifest.info.game))?;
        let path = app_state.game_paths.get(&game_id).cloned()
            .ok_or_else(|| format!("{} path not set. Configure it before restoring this backup.", app_state.game_label(&game_id)))?;
        if !Path::new(&path).is_dir() {
            return Err(crate::commands::files::missing_folder_error(&app_state.game_label(&game_id), &path));
        }
        let cts = get_game_def(&app_state.game_registry, &game_id)
            .map(|def| def.content_types.clone())
            .unwrap_or_default();
        (path, game_id, cts)
    };

    // Build category -> directory mapping from content types
    let mut category_dir_map: std::collections::HashMap<String, std::path::PathBuf> = std::collections::HashMap::new();
    for ct in &content_types {
        category_dir_map.insert(ct.id.clone(), std::path::PathBuf::from(&base).join(&ct.folder));
    }
    // Also support legacy category names (mods, saves, tray, screenshots)
    if !category_dir_map.contains_key("mods") {
        if let Some(ct) = content_types.first() {
            category_dir_map.insert("mods".to_string(), std::path::PathBuf::from(&base).join(&ct.folder));
        }
    }

    // Create safety backup first
    let safety_label = format!("Pre-restore safety backup ({})", manifest.info.label);
    let safety_id = Uuid::new_v4().to_string();
    let safety_dir = utils::backups_dir().join(&safety_id);
    std::fs::create_dir_all(&safety_dir).map_err(|e| e.to_string())?;

    let ct_for = |category: &str| -> Option<&ContentType> {
        content_types
            .iter()
            .find(|c| c.id == category)
            .or_else(|| if category == "mods" { content_types.first() } else { None })
    };
    let safety_total: usize = category_dir_map.iter().map(|(cat, dir)| count_files(dir, ct_for(cat))).sum();
    let mut safety_done = 0usize;
    let mut safety_entries = Vec::new();
    let mut safety_category_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (category, dir) in &category_dir_map {
        // The safety backup is the only way back if the restore goes wrong, so a
        // failure here must abort the restore. (Previously the error was swallowed
        // with unwrap_or_default(), recording an incomplete safety backup and then
        // overwriting the user's files anyway.)
        let entries = match copy_dir_to_backup(dir, ct_for(category), &safety_dir, category, &app, &mut safety_done, safety_total) {
            Ok(e) => e,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&safety_dir);
                return Err(format!("Safety backup failed, restore aborted: {}", e));
            }
        };
        safety_category_counts.insert(category.clone(), entries.len());
        safety_entries.extend(entries);
    }

    let safety_size: u64 = safety_entries.iter().map(|e| e.size).sum();

    let safety_info = BackupInfo {
        id: safety_id.clone(),
        created_at: utils::timestamp_now(),
        label: safety_label,
        file_count: safety_entries.len(),
        total_size: safety_size,
        mods_count: *safety_category_counts.get("mods").unwrap_or(&0),
        saves_count: *safety_category_counts.get("saves").unwrap_or(&0),
        tray_count: *safety_category_counts.get("tray").unwrap_or(&0),
        screenshots_count: *safety_category_counts.get("screenshots").unwrap_or(&0),
        game: game_id,
        auto: false,
    };
    let safety_manifest = BackupManifest {
        info: safety_info,
        files: safety_entries,
    };
    let sm_data = serde_json::to_string_pretty(&safety_manifest).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::write(safety_dir.join("manifest.json"), sm_data) {
        let _ = std::fs::remove_dir_all(&safety_dir);
        return Err(format!("Safety backup failed, restore aborted: {}", e));
    }

    // Restore files
    let mods_dir = content_types.first().map(|ct| std::path::PathBuf::from(&base).join(&ct.folder));
    let mut result = RestoreResult::default();
    let total = manifest.files.len();
    for (i, entry) in manifest.files.iter().enumerate() {
        if !is_plain_relative_path(&entry.relative_path) {
            continue;
        }

        let source = backup_dir.join(&entry.category).join(&entry.relative_path);
        let dest_base = match category_dir_map.get(&entry.category) {
            Some(dir) => dir,
            None => continue,
        };
        let dest = dest_base.join(&entry.relative_path);

        if let Some(twin) = disabled_twin(&dest, dest_base, mods_dir.as_deref(), &entry.relative_path) {
            result.skipped.push(format!("{} ({} exists)", entry.relative_path, twin));
        } else if source.exists() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(&source, &dest).map_err(|e| e.to_string())?;
            result.restored += 1;
        }

        let _ = app.emit(
            "restore-progress",
            serde_json::json!({
                "file": entry.relative_path,
                "files_done": i + 1,
                "files_total": total,
            }),
        );
    }

    Ok(result)
}

#[derive(Debug, Default, Serialize)]
pub struct RestoreResult {
    pub restored: usize,
    /// Files left alone because the user has since disabled/enabled them.
    pub skipped: Vec<String>,
}

/// The other state of a backed-up file, if it exists now: restoring
/// `x.package` next to a disabled `x.package.disabled` (or `_Disabled/x`)
/// silently re-enabled a mod the user had turned off, as a duplicate; the
/// reverse for a backed-up `x.disabled` next to an enabled `x`.
fn disabled_twin(
    dest: &Path,
    dest_base: &Path,
    mods_dir: Option<&Path>,
    rel: &str,
) -> Option<String> {
    let suffix = crate::commands::files::DISABLED_SUFFIX;
    let name = dest.file_name()?.to_string_lossy().to_string();
    let twin = if name.to_lowercase().ends_with(suffix) {
        dest.with_file_name(&name[..name.len() - suffix.len()])
    } else {
        dest.with_file_name(format!("{}{}", name, suffix))
    };
    if twin.exists() {
        return twin.file_name().map(|n| n.to_string_lossy().to_string());
    }
    if mods_dir == Some(dest_base) {
        let legacy = dest_base.join("_Disabled").join(rel);
        if legacy.exists() {
            return Some(format!("_Disabled/{}", rel));
        }
    }
    None
}

#[tauri::command]
pub async fn rename_backup(id: String, label: String) -> Result<(), String> {
    utils::sanitize_id(&id)?;

    let label = label.trim().to_string();
    if label.is_empty() || label.len() > MAX_BACKUP_LABEL_LEN {
        return Err(format!("Label must be 1-{MAX_BACKUP_LABEL_LEN} characters"));
    }
    if label.chars().any(|c| c.is_control()) {
        return Err("Label contains invalid characters".into());
    }

    let backup_dir = utils::backups_dir().join(&id);
    let manifest_path = backup_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err("Backup not found".into());
    }

    let data = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let mut manifest: BackupManifest = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    manifest.info.label = label;

    let updated = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, updated).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn delete_backup(id: String) -> Result<(), String> {
    utils::sanitize_id(&id)?;

    let backup_dir = utils::backups_dir().join(&id);
    if !backup_dir.exists() {
        return Err("Backup not found".into());
    }

    let canonical = std::fs::canonicalize(&backup_dir).map_err(|e| e.to_string())?;
    let backups_canonical = std::fs::canonicalize(utils::backups_dir()).map_err(|e| e.to_string())?;
    if !canonical.starts_with(&backups_canonical) {
        return Err("Invalid backup path".into());
    }

    std::fs::remove_dir_all(&backup_dir).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_skips_files_the_user_disabled_or_enabled() {
        let base = std::env::temp_dir().join(format!("synccrate-restore-{}", uuid::Uuid::new_v4()));
        let mods = base.join("Mods");
        std::fs::create_dir_all(mods.join("_Disabled").join("CC")).unwrap();
        std::fs::write(mods.join("a.package.disabled"), b"x").unwrap();
        std::fs::write(mods.join("b.package"), b"x").unwrap();
        std::fs::write(mods.join("_Disabled").join("CC").join("c.package"), b"x").unwrap();

        let twin = |rel: &str| disabled_twin(&mods.join(rel), &mods, Some(&mods), rel);
        assert_eq!(twin("a.package").as_deref(), Some("a.package.disabled"));
        assert_eq!(twin("b.package.disabled").as_deref(), Some("b.package"));
        assert_eq!(twin("CC/c.package").as_deref(), Some("_Disabled/CC/c.package"));
        assert_eq!(twin("d.package"), None);
        // `_Disabled` only means something in the mods folder.
        let saves = base.join("Saves");
        assert_eq!(disabled_twin(&saves.join("CC/c.package"), &saves, Some(&mods), "CC/c.package"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plain_relative_paths_only() {
        assert!(is_plain_relative_path("sub/file.package"));
        assert!(is_plain_relative_path("file.save"));
        assert!(!is_plain_relative_path(""));
        assert!(!is_plain_relative_path("../escape"));
        assert!(!is_plain_relative_path("a/../../escape"));
        #[cfg(target_os = "windows")]
        {
            assert!(!is_plain_relative_path(r"\Windows\evil.dll"));
            assert!(!is_plain_relative_path("C:evil"));
            assert!(!is_plain_relative_path(r"C:\evil"));
        }
        #[cfg(not(target_os = "windows"))]
        assert!(!is_plain_relative_path("/etc/passwd"));
    }
}
