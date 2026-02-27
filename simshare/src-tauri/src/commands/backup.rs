use crate::state::{AppState, SimsGame};
use crate::utils;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;
use uuid::Uuid;
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
}

fn default_game() -> String {
    "Sims4".to_string()
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
    pub category: String, // "mods" or "saves"
}

fn copy_dir_to_backup(
    source_dir: &Path,
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

    for entry in WalkDir::new(source_dir)
        .follow_links(false) // Security: don't follow symlinks
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        // Security: skip symlinks
        if entry.path_is_symlink() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(source_dir)
            .map_err(|e| e.to_string())?;

        // Validate no path traversal in relative path
        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue; // skip files with '..' in path
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

fn count_files(dir: &Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && !e.path_is_symlink())
        .count()
}

fn parse_game(game: &str) -> Result<SimsGame, String> {
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

#[tauri::command]
pub async fn create_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    label: String,
    game: Option<String>,
) -> Result<BackupInfo, String> {
    // Validate label
    let label = label.trim().to_string();
    if label.is_empty() || label.len() > MAX_BACKUP_LABEL_LEN {
        return Err(format!("Label must be 1-{MAX_BACKUP_LABEL_LEN} characters"));
    }
    // Sanitize: no control characters
    if label.chars().any(|c| c.is_control()) {
        return Err("Label contains invalid characters".into());
    }

    let (base, target_game) = {
        let app_state = state.lock().await;
        let target_game = match game {
            Some(ref g) => parse_game(g)?,
            None => app_state.active_game.clone(),
        };
        let path = app_state.game_paths.get(&target_game).cloned()
            .ok_or_else(|| format!("{} path not set", utils::game_label(&target_game)))?;
        (path, target_game)
    };

    let mods_dir = utils::mods_path(&base);
    let saves_dir = utils::saves_path(&base);
    let tray_dir = utils::tray_path(&base);
    let screenshots_dir = utils::screenshots_path(&base);
    let files_total = count_files(&mods_dir) + count_files(&saves_dir) + count_files(&tray_dir) + count_files(&screenshots_dir);

    let id = Uuid::new_v4().to_string();
    let backup_dir = utils::backups_dir().join(&id);
    std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;

    let mut files_done = 0usize;
    let mut all_entries = Vec::new();

    let mods_entries =
        copy_dir_to_backup(&mods_dir, &backup_dir, "mods", &app, &mut files_done, files_total)?;
    let saves_entries =
        copy_dir_to_backup(&saves_dir, &backup_dir, "saves", &app, &mut files_done, files_total)?;
    let tray_entries =
        copy_dir_to_backup(&tray_dir, &backup_dir, "tray", &app, &mut files_done, files_total)?;
    let screenshots_entries =
        copy_dir_to_backup(&screenshots_dir, &backup_dir, "screenshots", &app, &mut files_done, files_total)?;

    let mods_count = mods_entries.len();
    let saves_count = saves_entries.len();
    let tray_count = tray_entries.len();
    let screenshots_count = screenshots_entries.len();
    all_entries.extend(mods_entries);
    all_entries.extend(saves_entries);
    all_entries.extend(tray_entries);
    all_entries.extend(screenshots_entries);

    let total_size: u64 = all_entries.iter().map(|e| e.size).sum();

    let info = BackupInfo {
        id: id.clone(),
        created_at: utils::timestamp_now(),
        label,
        file_count: all_entries.len(),
        total_size,
        mods_count,
        saves_count,
        tray_count,
        screenshots_count,
        game: game_to_string(&target_game),
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

#[tauri::command]
pub async fn restore_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    utils::sanitize_id(&id)?;

    let backup_dir = utils::backups_dir().join(&id);
    let manifest_path = backup_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err("Backup not found".into());
    }

    let data = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: BackupManifest = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    // Resolve game from backup manifest, fall back to active game
    let backup_game = parse_game(&manifest.info.game).unwrap_or(SimsGame::Sims4);
    let base = {
        let app_state = state.lock().await;
        app_state.game_paths.get(&backup_game).cloned()
            .ok_or_else(|| format!("{} path not set. Configure it before restoring this backup.", utils::game_label(&backup_game)))?
    };

    // Create safety backup first
    let safety_label = format!("Pre-restore safety backup ({})", manifest.info.label);
    let safety_id = Uuid::new_v4().to_string();
    let safety_dir = utils::backups_dir().join(&safety_id);
    std::fs::create_dir_all(&safety_dir).map_err(|e| e.to_string())?;

    let mods_dir = utils::mods_path(&base);
    let saves_dir = utils::saves_path(&base);
    let tray_dir = utils::tray_path(&base);
    let screenshots_dir = utils::screenshots_path(&base);
    let safety_total = count_files(&mods_dir) + count_files(&saves_dir) + count_files(&tray_dir) + count_files(&screenshots_dir);
    let mut safety_done = 0usize;
    let mut safety_entries = Vec::new();

    let mods_e = copy_dir_to_backup(&mods_dir, &safety_dir, "mods", &app, &mut safety_done, safety_total)
        .unwrap_or_default();
    let saves_e = copy_dir_to_backup(&saves_dir, &safety_dir, "saves", &app, &mut safety_done, safety_total)
        .unwrap_or_default();
    let tray_e = copy_dir_to_backup(&tray_dir, &safety_dir, "tray", &app, &mut safety_done, safety_total)
        .unwrap_or_default();
    let screenshots_e = copy_dir_to_backup(&screenshots_dir, &safety_dir, "screenshots", &app, &mut safety_done, safety_total)
        .unwrap_or_default();

    let safety_mods = mods_e.len();
    let safety_saves = saves_e.len();
    let safety_tray = tray_e.len();
    let safety_screenshots = screenshots_e.len();
    safety_entries.extend(mods_e);
    safety_entries.extend(saves_e);
    safety_entries.extend(tray_e);
    safety_entries.extend(screenshots_e);
    let safety_size: u64 = safety_entries.iter().map(|e| e.size).sum();

    let safety_info = BackupInfo {
        id: safety_id.clone(),
        created_at: utils::timestamp_now(),
        label: safety_label,
        file_count: safety_entries.len(),
        total_size: safety_size,
        mods_count: safety_mods,
        saves_count: safety_saves,
        tray_count: safety_tray,
        screenshots_count: safety_screenshots,
        game: game_to_string(&backup_game),
    };
    let safety_manifest = BackupManifest {
        info: safety_info,
        files: safety_entries,
    };
    let sm_data = serde_json::to_string_pretty(&safety_manifest).map_err(|e| e.to_string())?;
    std::fs::write(safety_dir.join("manifest.json"), sm_data).map_err(|e| e.to_string())?;

    // Restore files
    let total = manifest.files.len();
    for (i, entry) in manifest.files.iter().enumerate() {
        // Validate path - no traversal
        let rel = Path::new(&entry.relative_path);
        if rel.is_absolute() {
            continue;
        }
        let has_traversal = rel.components().any(|c| matches!(c, std::path::Component::ParentDir));
        if has_traversal {
            continue;
        }

        // Validate category is one of the known values
        if !matches!(entry.category.as_str(), "mods" | "saves" | "tray" | "screenshots") {
            continue;
        }

        let source = backup_dir.join(&entry.category).join(&entry.relative_path);
        let dest_base = match entry.category.as_str() {
            "mods" => &mods_dir,
            "saves" => &saves_dir,
            "tray" => &tray_dir,
            "screenshots" => &screenshots_dir,
            _ => continue,
        };
        let dest = dest_base.join(&entry.relative_path);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        if source.exists() {
            std::fs::copy(&source, &dest).map_err(|e| e.to_string())?;
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

    Ok(())
}

#[tauri::command]
pub async fn delete_backup(id: String) -> Result<(), String> {
    utils::sanitize_id(&id)?;

    let backup_dir = utils::backups_dir().join(&id);
    if !backup_dir.exists() {
        return Err("Backup not found".into());
    }

    // Verify it's inside backups dir
    let canonical = std::fs::canonicalize(&backup_dir).map_err(|e| e.to_string())?;
    let backups_canonical = std::fs::canonicalize(utils::backups_dir()).map_err(|e| e.to_string())?;
    if !canonical.starts_with(&backups_canonical) {
        return Err("Invalid backup path".into());
    }

    std::fs::remove_dir_all(&backup_dir).map_err(|e| e.to_string())
}
