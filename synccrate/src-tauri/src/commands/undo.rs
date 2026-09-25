//! "Undo last sync": one record per game, next to the backup store, naming
//! the presync backup (if any) and exactly which files the sync added,
//! replaced or deleted. `commands::backup::undo_apply` does the actual file
//! work; this module owns the record's lifecycle (write at the end of a
//! sync, read/invalidate for the UI, delete once undone) and the command's
//! guards, which mirror `restore_backup`'s.
use crate::commands::files::{get_game_def, resolve_game};
use crate::commands::{backup, game_state};
use crate::state::AppState;
use crate::utils;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordedFile {
    pub(crate) relative_path: String,
    pub(crate) size: u64,
    pub(crate) mtime_ms: i64,
    pub(crate) hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SyncRecord {
    pub(crate) sync_id: String,
    pub(crate) created_at: u64,
    pub(crate) game: String,
    pub(crate) base_path: String,
    /// `None` when the sync replaced or deleted nothing, so no presync
    /// backup was made (or "back up before sync" was off).
    pub(crate) presync_backup_id: Option<String>,
    /// Files this sync wrote that didn't exist before (plain downloads and
    /// "keep both" `_remote` copies) — undone by deleting them.
    pub(crate) added: Vec<RecordedFile>,
    /// Files this sync overwrote via "use theirs" — undone by restoring the
    /// presync backup's copy.
    pub(crate) replaced: Vec<RecordedFile>,
    /// Files this sync deleted — undone by restoring the presync backup's copy.
    pub(crate) deleted: Vec<String>,
}

impl SyncRecord {
    pub(crate) fn is_empty(&self) -> bool {
        self.added.is_empty() && self.replaced.is_empty() && self.deleted.is_empty()
    }
}

fn records_dir() -> PathBuf {
    let dir = utils::config_root().join("synccrate").join("sync_records");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Game ids come from the registry (already `[a-z0-9_]`-shaped and tested
/// unique), but this is a filename, so filter defensively rather than trust it.
fn record_path(game: &str) -> PathBuf {
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    records_dir().join(format!("{}.json", if safe.is_empty() { "unknown".to_string() } else { safe }))
}

pub(crate) fn read_record(game: &str) -> Option<SyncRecord> {
    let data = std::fs::read_to_string(record_path(game)).ok()?;
    serde_json::from_str(&data).ok()
}

/// Overwrite `game`'s record. Only one sync's worth is ever kept.
pub(crate) fn write_record(record: &SyncRecord) {
    let path = record_path(&record.game);
    let Ok(data) = serde_json::to_string_pretty(record) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub(crate) fn delete_record(game: &str) {
    let _ = std::fs::remove_file(record_path(game));
}

/// The presync backup id the game's current record points to, if any —
/// `create_presync_backup` protects it from pruning.
pub(crate) fn protected_presync_id(game: &str) -> Option<String> {
    read_record(game).and_then(|r| r.presync_backup_id)
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoStatus {
    pub created_at: u64,
    pub added: usize,
    pub replaced: usize,
    pub deleted: usize,
}

/// For the UI: whether there's a sync to undo for `game` (default: the
/// active game), and a summary to display. `None` once the record is gone,
/// already undone, or invalidated by a game/folder change.
#[tauri::command]
pub async fn get_undo_status(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<Option<UndoStatus>, String> {
    let (game_id, base_path) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let base_path = app_state.game_paths.get(&game_id).cloned();
        (game_id, base_path)
    };
    Ok(read_record(&game_id)
        .filter(|r| Some(&r.base_path) == base_path.as_ref())
        .map(|r| UndoStatus {
            created_at: r.created_at,
            added: r.added.len(),
            replaced: r.replaced.len(),
            deleted: r.deleted.len(),
        }))
}

/// Undo the last sync for `game` (default: the active game). Only the client
/// side of a sync writes a record — a host never has one, so this is
/// naturally a no-op there.
#[tauri::command]
pub async fn undo_last_sync(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<backup::UndoResult, String> {
    undo_last_sync_inner(state.inner(), game).await
}

pub(crate) async fn undo_last_sync_inner(
    state: &Arc<Mutex<AppState>>,
    game: Option<String>,
) -> Result<backup::UndoResult, String> {
    let (game_id, base_path, content_types, process_names, game_label) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        if app_state.is_any_syncing() {
            return Err("Can't undo while a sync is in progress.".to_string());
        }
        // A host may be serving these files to someone, so never rewrite them
        // there (restore_backup's rule). A connected client can undo: the
        // restore flag below stops a new sync (or a stay-in-sync pull) from
        // starting until it's done, and the "Undo" toast right after a sync
        // is shown while still connected.
        if app_state.session_type == crate::state::SessionType::Host {
            return Err("Stop hosting before undoing a sync.".to_string());
        }
        let game_label = app_state.game_label(&game_id);
        let base_path = app_state.game_paths.get(&game_id).cloned();
        let def = get_game_def(&app_state.game_registry, &game_id);
        let cts = def.map(|d| d.content_types.clone()).unwrap_or_default();
        let procs = def.map(|d| d.process_names.clone()).unwrap_or_default();
        (game_id, base_path, cts, procs, game_label)
    };

    let Some(record) = read_record(&game_id) else {
        return Err("Nothing to undo.".to_string());
    };
    let Some(base_path) = base_path else {
        return Err(format!("{} path not set.", game_label));
    };
    if record.game != game_id || record.base_path != base_path {
        return Err("The game or folder changed since that sync — nothing to undo.".to_string());
    }
    if !std::path::Path::new(&base_path).is_dir() {
        return Err(crate::commands::files::missing_folder_error(&game_label, &base_path));
    }

    if game_state::is_game_running(&process_names).await.unwrap_or(false) {
        return Err(format!("{} is running. Close the game before undoing a sync.", game_label));
    }

    // Shares `restore_backup`'s flag: a restore can't start mid-undo, and a
    // second undo (or a concurrent restore) is refused rather than racing it.
    let Some(_guard) = backup::try_begin_restoring() else {
        return Err("A restore is already running.".to_string());
    };

    let base = std::path::PathBuf::from(&base_path);
    let result = tokio::task::spawn_blocking(move || backup::undo_apply(&record, &base, &content_types))
        .await
        .map_err(|e| e.to_string())?;

    delete_record(&game_id);
    // Any plan on screen was computed against the files as they were.
    for conn in state.lock().await.connections.values_mut() {
        conn.sync_plan = None;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> RecordedFile {
        RecordedFile { relative_path: path.to_string(), size: 4, mtime_ms: 1000, hash: "h".to_string() }
    }

    #[test]
    fn is_empty_true_only_with_no_files_at_all() {
        let mut r = SyncRecord {
            sync_id: "s".into(), created_at: 0, game: "g".into(), base_path: "b".into(),
            presync_backup_id: None, added: vec![], replaced: vec![], deleted: vec![],
        };
        assert!(r.is_empty());
        r.added.push(file("Mods/a.package"));
        assert!(!r.is_empty());
        r.added.clear();
        r.deleted.push("Mods/gone.package".into());
        assert!(!r.is_empty());
    }

    // Touches the redirected config dir (`SYNCCRATE_CONFIG_DIR`), which is
    // process-global, so this holds the same lock the E2E suite uses to keep
    // it from racing other tests that touch it.
    #[tokio::test]
    async fn record_path_sanitizes_and_roundtrips() {
        let _g = crate::testutil::e2e_guard().await;
        let record = SyncRecord {
            sync_id: "abc".into(), created_at: 123, game: "sims4".into(), base_path: "C:/Game".into(),
            presync_backup_id: Some("bkp1".into()), added: vec![file("Mods/new.package")],
            replaced: vec![], deleted: vec!["Mods/gone.package".into()],
        };
        write_record(&record);
        let read = read_record("sims4").expect("record round-trips");
        assert_eq!(read.sync_id, "abc");
        assert_eq!(read.added.len(), 1);
        assert_eq!(protected_presync_id("sims4"), Some("bkp1".to_string()));
        delete_record("sims4");
        assert!(read_record("sims4").is_none());
    }
}
