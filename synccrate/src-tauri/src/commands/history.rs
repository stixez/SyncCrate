//! Per-file version history: the previous versions of files a sync replaced
//! or deleted, so one bad mod update can be rolled back without restoring a
//! whole backup ("updated by Alex on Friday" → restore that version).
//!
//! Versions live in the backup store's deduplicated object store (`objects/`,
//! keyed by sha256), indexed per game in `<backups>/history/<game>.json`.
//! `backup::gc_objects` treats every hash in these indexes as referenced.
//!
//! Lifecycle: before a sync, `begin_capture` copies every local file the plan
//! will overwrite or delete into the store and records them as *pending*
//! (so a backup's GC during the sync can't collect them); `finish` then keeps
//! only the ones the sync actually replaced or deleted. Leftover pending
//! entries (the app died mid-sync) are dropped by `prune` after a day.
//! Independent of "back up before sync" and best-effort: a failed capture is
//! logged and the sync goes ahead (that setting is the strict option).
use crate::commands::backup;
use crate::commands::files::resolve_game;
use crate::state::AppState;
use crate::utils;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const HISTORY_DIR: &str = "history";
const FORMAT_VERSION: u32 = 1;
pub const MAX_VERSIONS_PER_FILE: usize = 10;
pub const MAX_AGE_SECS: u64 = 180 * 24 * 3600;
/// Unique bytes kept per game; the oldest versions go first past this.
pub const MAX_BYTES_PER_GAME: u64 = 2 * 1024 * 1024 * 1024;
const PENDING_TTL_SECS: u64 = 24 * 3600;

pub const REASON_REPLACED: &str = "replaced";
pub const REASON_DELETED: &str = "deleted";
/// The file as it was right before a version was restored over it.
pub const REASON_BEFORE_RESTORE: &str = "before-restore";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileVersion {
    pub id: String,
    /// Game-folder-relative path, forward slashes.
    pub path: String,
    pub hash: String,
    pub size: u64,
    #[serde(default)]
    pub mtime_ms: Option<i64>,
    /// When this version stopped being the current one (unix secs).
    pub at: u64,
    pub reason: String,
    /// Who the new version came from (the host's name), if a sync.
    #[serde(default)]
    pub peer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryFile {
    version: u32,
    entries: Vec<FileVersion>,
}

fn safe_game(game: &str) -> String {
    let s: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    if s.is_empty() { "unknown".into() } else { s }
}

fn history_path(root: &Path, game: &str) -> PathBuf {
    root.join(HISTORY_DIR).join(format!("{}.json", safe_game(game)))
}

/// Missing → empty. Unreadable → error, so nothing overwrites (or GCs past) it.
fn load(root: &Path, game: &str) -> Result<HistoryFile, String> {
    let path = history_path(root, game);
    match std::fs::read(&path) {
        Ok(raw) => {
            let h: HistoryFile = serde_json::from_slice(&raw).map_err(|e| format!("{}: {}", path.display(), e))?;
            if h.version > FORMAT_VERSION {
                return Err(format!("{} was written by a newer SyncCrate", path.display()));
            }
            Ok(h)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HistoryFile { version: FORMAT_VERSION, entries: vec![] }),
        Err(e) => Err(format!("{}: {}", path.display(), e)),
    }
}

fn save(root: &Path, game: &str, h: &HistoryFile) -> Result<(), String> {
    let path = history_path(root, game);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(h).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Every object hash any history index references (for `backup::gc_objects`).
/// An unreadable index is an error: better to skip a GC than delete versions.
pub(crate) fn referenced_hashes(root: &Path) -> Result<HashSet<String>, String> {
    let mut out = HashSet::new();
    let Ok(dir) = std::fs::read_dir(root.join(HISTORY_DIR)) else { return Ok(out) };
    for e in dir.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        let h: HistoryFile = serde_json::from_slice(&raw).map_err(|e| format!("{}: {}", p.display(), e))?;
        out.extend(h.entries.into_iter().map(|e| e.hash));
    }
    Ok(out)
}

/// Retention, newest first wins: stale pending entries go, then anything past
/// the age limit, then past the per-file count, then the oldest versions
/// until the game's unique bytes fit. Keeps `entries` oldest-first (append
/// order breaks ties, since several versions can share a second). Returns
/// whether anything was removed.
fn prune(entries: &mut Vec<FileVersion>, now: u64) -> bool {
    let before = entries.len();
    entries.sort_by_key(|e| e.at);
    entries.retain(|e| match &e.pending {
        Some(_) => now.saturating_sub(e.at) < PENDING_TTL_SECS,
        None => now.saturating_sub(e.at) < MAX_AGE_SECS,
    });
    let mut keep = vec![true; entries.len()];
    let mut per_file: HashMap<String, usize> = HashMap::new();
    let mut seen = HashSet::new();
    let mut bytes = 0u64;
    for (i, e) in entries.iter().enumerate().rev() {
        let n = per_file.entry(e.path.to_lowercase()).or_default();
        *n += 1;
        if *n > MAX_VERSIONS_PER_FILE {
            keep[i] = false;
            continue;
        }
        if seen.insert(e.hash.clone()) {
            bytes += e.size;
        }
        // The newest version always stays, even if it alone is over the cap.
        if bytes > MAX_BYTES_PER_GAME && i + 1 != entries.len() {
            keep[i] = false;
        }
    }
    let mut it = keep.into_iter();
    entries.retain(|_| it.next().unwrap_or(true));
    entries.len() != before
}

/// Copy the files a sync is about to replace/delete into the store and index
/// them as pending under `capture_id`. Files that don't exist (or aren't
/// regular files) are skipped. Blocking; call via `spawn_blocking`.
pub(crate) fn begin_capture(root: &Path, game: &str, base: &str, targets: &[String], peer: &str, capture_id: &str, now: u64) -> Result<usize, String> {
    let _lock = backup::store_lock();
    let mut h = load(root, game)?;
    let mut n = 0;
    let mut seen = HashSet::new();
    for rel in targets {
        if !seen.insert(rel.to_lowercase()) {
            continue;
        }
        let Ok(abs) = utils::safe_join(base, rel) else { continue };
        let Ok(meta) = std::fs::symlink_metadata(&abs) else { continue };
        if !meta.file_type().is_file() {
            continue;
        }
        match backup::store_object(root, &abs) {
            Ok((hash, size, _)) => {
                h.entries.push(FileVersion {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: rel.replace('\\', "/"),
                    hash,
                    size,
                    mtime_ms: backup::mtime_ms(&meta),
                    at: now,
                    reason: REASON_REPLACED.into(),
                    peer: peer.to_string(),
                    pending: Some(capture_id.to_string()),
                });
                n += 1;
            }
            Err(e) => log::warn!("File history: couldn't keep {}: {}", rel, e),
        }
    }
    if n > 0 {
        save(root, game, &h)?;
    }
    Ok(n)
}

/// `begin_capture` when "back up before sync" just stored exactly these files:
/// index the presync backup's objects instead of copying and hashing them
/// again. Targets the backup doesn't hold (it skipped them) are copied as usual.
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_capture_from_backup(
    root: &Path,
    game: &str,
    base: &str,
    backup_id: &str,
    cts: &[crate::registry::ContentType],
    targets: &[String],
    peer: &str,
    capture_id: &str,
    now: u64,
) -> Result<usize, String> {
    let (n, missing) = {
        let _lock = backup::store_lock();
        let objects = backup::backup_objects(root, backup_id, cts)?;
        let by_path: HashMap<String, &(String, String, u64, Option<i64>)> = objects.iter().map(|o| (o.0.to_lowercase(), o)).collect();
        let mut h = load(root, game)?;
        let mut n = 0;
        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        for rel in targets {
            let key = rel.replace('\\', "/").to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            match by_path.get(&key) {
                Some((path, hash, size, mtime)) => {
                    h.entries.push(FileVersion {
                        id: uuid::Uuid::new_v4().to_string(),
                        path: path.clone(),
                        hash: hash.clone(),
                        size: *size,
                        mtime_ms: *mtime,
                        at: now,
                        reason: REASON_REPLACED.into(),
                        peer: peer.to_string(),
                        pending: Some(capture_id.to_string()),
                    });
                    n += 1;
                }
                None => missing.push(rel.clone()),
            }
        }
        if n > 0 {
            save(root, game, &h)?;
        }
        (n, missing)
    };
    if missing.is_empty() {
        return Ok(n);
    }
    Ok(n + begin_capture(root, game, base, &missing, peer, capture_id, now)?)
}

/// After the sync: keep the pending versions of files it really replaced or
/// deleted (marking deletions as such), drop the rest, apply retention.
pub(crate) fn finish(root: &Path, game: &str, capture_id: &str, replaced: &[String], deleted: &[String], now: u64) -> Result<(), String> {
    let _lock = backup::store_lock();
    let mut h = load(root, game)?;
    let replaced: HashSet<String> = replaced.iter().map(|p| p.to_lowercase()).collect();
    let deleted: HashSet<String> = deleted.iter().map(|p| p.to_lowercase()).collect();
    let before = h.entries.len();
    h.entries.retain_mut(|e| {
        if e.pending.as_deref() != Some(capture_id) {
            return true;
        }
        let key = e.path.to_lowercase();
        if deleted.contains(&key) {
            e.reason = REASON_DELETED.into();
        } else if !replaced.contains(&key) {
            return false;
        }
        e.pending = None;
        true
    });
    // Versions the sync didn't replace after all (cancelled, or refused as
    // changed locally) were dropped above; their objects need GC too, or they
    // leaked for anyone without backups (the only other GC trigger).
    let dropped = h.entries.len() < before;
    let pruned = prune(&mut h.entries, now);
    save(root, game, &h)?;
    // GC must run under the store lock, like every other caller: unlocked,
    // it would delete a concurrent backup's objects/tmp files and objects no
    // manifest references yet.
    if pruned || dropped {
        backup::gc_logged(root);
    }
    Ok(())
}

/// Versions for a game (optionally one path, case-insensitive), newest first.
fn list(root: &Path, game: &str, path: Option<&str>) -> Result<Vec<FileVersion>, String> {
    let mut v: Vec<FileVersion> = load(root, game)?
        .entries
        .into_iter()
        .rev()
        .filter(|e| e.pending.is_none())
        .filter(|e| path.map_or(true, |p| e.path.eq_ignore_ascii_case(p)))
        .collect();
    // Stable: same-second versions stay newest-appended first.
    v.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(v)
}

/// Put one version back. The current file (if any) is kept as a version
/// first, so a restore can itself be undone. Blocking.
fn restore(root: &Path, game: &str, base: &str, id: &str, now: u64) -> Result<FileVersion, String> {
    let _lock = backup::store_lock();
    let mut h = load(root, game)?;
    let v = h.entries.iter().find(|e| e.id == id && e.pending.is_none()).cloned().ok_or("That version is no longer kept.")?;
    if !backup::is_valid_hash(&v.hash) {
        return Err("That version is damaged.".into());
    }
    let object = backup::object_path(root, &v.hash);
    if !object.is_file() {
        return Err("That version's data is missing from the backup store.".into());
    }
    let dest = utils::safe_join(base, &v.path)?;
    if let Ok(meta) = std::fs::symlink_metadata(&dest) {
        if !meta.file_type().is_file() {
            return Err(format!("{} isn't a regular file; not replacing it.", v.path));
        }
        let (hash, size, _) = backup::store_object(root, &dest)?;
        if hash == v.hash {
            return Err("That version is already the current file.".into());
        }
        h.entries.push(FileVersion {
            id: uuid::Uuid::new_v4().to_string(),
            path: v.path.clone(),
            hash,
            size,
            mtime_ms: backup::mtime_ms(&meta),
            at: now,
            reason: REASON_BEFORE_RESTORE.into(),
            peer: String::new(),
            pending: None,
        });
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Copy next to the target, then rename over it: a failed copy never
    // leaves a half-written mod in place.
    let tmp = dest.with_file_name(format!(".{}.synccrate-restore.tmp", uuid::Uuid::new_v4()));
    let copied = std::fs::copy(&object, &tmp).map_err(|e| e.to_string()).and_then(|_| {
        if let Some(ms) = v.mtime_ms {
            let _ = backup::set_mtime(&tmp, ms);
        }
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Couldn't restore {}: {}", v.path, e));
    }
    let pruned = prune(&mut h.entries, now);
    save(root, game, &h)?;
    if pruned {
        backup::gc_logged(root);
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Commands

#[tauri::command]
pub async fn list_file_history(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
    path: Option<String>,
) -> Result<Vec<FileVersion>, String> {
    let game = resolve_game(&*state.lock().await, &game)?;
    tokio::task::spawn_blocking(move || list(&utils::backups_dir(), &game, path.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_file_version(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
    id: String,
) -> Result<FileVersion, String> {
    restore_file_version_inner(state.inner(), &game, &id).await
}

pub(crate) async fn restore_file_version_inner(state: &Arc<Mutex<AppState>>, game: &str, id: &str) -> Result<FileVersion, String> {
    let (game, base, label, procs) = {
        let s = state.lock().await;
        let game = resolve_game(&s, game)?;
        if s.is_any_syncing() {
            return Err("A sync is running. Restore the version when it's done.".into());
        }
        let base = s.game_paths.get(&game).cloned().ok_or_else(|| format!("{} path not set.", s.game_label(&game)))?;
        let procs = s.game_registry.games.iter().find(|g| g.id == game).map(|g| g.process_names.clone()).unwrap_or_default();
        (game.clone(), base, s.game_label(&game), procs)
    };
    if !Path::new(&base).is_dir() {
        return Err(crate::commands::files::missing_folder_error(&label, &base));
    }
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return Err(format!("{} is running. Close the game before restoring a file.", label));
    }
    let _guard = backup::try_begin_restoring().ok_or("A restore is already running.")?;
    let id = id.to_string();
    tokio::task::spawn_blocking(move || restore(&utils::backups_dir(), &game, &base, &id, utils::timestamp_now()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(path: &str, hash: &str, size: u64, at: u64) -> FileVersion {
        FileVersion { id: uuid::Uuid::new_v4().to_string(), path: path.into(), hash: hash.into(), size, mtime_ms: None, at, reason: REASON_REPLACED.into(), peer: String::new(), pending: None }
    }

    #[test]
    fn prune_applies_age_count_and_bytes_newest_first() {
        let now = MAX_AGE_SECS * 2;
        let mut e = vec![v("a", "h-old", 1, now - MAX_AGE_SECS - 1)];
        for i in 0..(MAX_VERSIONS_PER_FILE + 3) {
            e.push(v("Mods/X.package", &format!("h{i}"), 1, now - i as u64));
        }
        let stale = FileVersion { pending: Some("dead".into()), ..v("p", "hp", 1, now - PENDING_TTL_SECS - 1) };
        let live = FileVersion { pending: Some("live".into()), ..v("q", "hq", 1, now) };
        e.push(stale);
        e.push(live);
        assert!(prune(&mut e, now));
        assert!(!e.iter().any(|x| x.path == "a"), "past the age limit");
        assert!(!e.iter().any(|x| x.path == "p"), "a pending entry from a crashed sync");
        assert!(e.iter().any(|x| x.path == "q"), "a running sync's pending entry stays");
        let x: Vec<_> = e.iter().filter(|x| x.path == "Mods/X.package").collect();
        assert_eq!(x.len(), MAX_VERSIONS_PER_FILE);
        assert_eq!(x.last().unwrap().hash, "h0", "newest kept, stored oldest-first");

        // Byte cap: shared hashes count once; the oldest go first.
        let big = MAX_BYTES_PER_GAME / 2 + 1;
        let mut e = vec![v("a", "A", big, 30), v("b", "A", big, 20), v("c", "C", big, 10)];
        prune(&mut e, 40);
        let paths: Vec<_> = e.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, vec!["b", "a"], "a and b share one object; c would pass the cap");
    }

    fn setup() -> (PathBuf, PathBuf, String) {
        let root = crate::testutil::temp_dir("hist-root");
        let base = crate::testutil::temp_dir("hist-game");
        crate::testutil::write_file(&base, "Mods/a.package", b"V1");
        crate::testutil::write_file(&base, "Mods/gone.package", b"G1");
        crate::testutil::write_file(&base, "Mods/untouched.package", b"U1");
        let b = base.to_str().unwrap().to_string();
        (root, base, b)
    }

    #[test]
    fn capture_finish_keeps_only_what_the_sync_changed_and_restore_round_trips() {
        let (root, base, b) = setup();
        let targets: Vec<String> = ["Mods/a.package", "Mods/gone.package", "Mods/untouched.package", "Mods/missing.package", "../../evil"].iter().map(|s| s.to_string()).collect();
        assert_eq!(begin_capture(&root, "sims4", &b, &targets, "Alex", "cap1", 100).unwrap(), 3);
        assert!(list(&root, "sims4", None).unwrap().is_empty(), "pending versions aren't listed");
        assert!(referenced_hashes(&root).unwrap().contains(&crate::testutil::sha256_hex(b"V1")), "GC sees pending versions");

        // The sync replaced a, deleted gone, and never got to untouched.
        crate::testutil::write_file(&base, "Mods/a.package", b"V2");
        std::fs::remove_file(base.join("Mods/gone.package")).unwrap();
        finish(&root, "sims4", "cap1", &["Mods/a.package".into()], &["Mods/gone.package".into()], 101).unwrap();
        let all = list(&root, "sims4", None).unwrap();
        assert_eq!(all.len(), 2);
        let a = list(&root, "sims4", Some("mods/A.package")).unwrap();
        assert_eq!((a.len(), a[0].reason.as_str(), a[0].peer.as_str()), (1, REASON_REPLACED, "Alex"));
        assert_eq!(list(&root, "sims4", Some("Mods/gone.package")).unwrap()[0].reason, REASON_DELETED);

        // Roll a back: V1 returns, and V2 is kept as a version.
        restore(&root, "sims4", &b, &a[0].id, 200).unwrap();
        assert_eq!(std::fs::read(base.join("Mods/a.package")).unwrap(), b"V1");
        let a = list(&root, "sims4", Some("Mods/a.package")).unwrap();
        assert_eq!(a[0].reason, REASON_BEFORE_RESTORE);
        assert_eq!(a[0].hash, crate::testutil::sha256_hex(b"V2"));
        // Restoring what's already there is refused; so is an unknown id.
        let v1 = a.iter().find(|x| x.reason == REASON_REPLACED).unwrap();
        assert!(restore(&root, "sims4", &b, &v1.id, 201).unwrap_err().contains("already"));
        assert!(restore(&root, "sims4", &b, "nope", 201).is_err());
        // A deleted file comes back too.
        let gone = list(&root, "sims4", Some("Mods/gone.package")).unwrap();
        restore(&root, "sims4", &b, &gone[0].id, 202).unwrap();
        assert_eq!(std::fs::read(base.join("Mods/gone.package")).unwrap(), b"G1");
        assert!(walkdir::WalkDir::new(&base).into_iter().filter_map(|e| e.ok()).all(|e| !e.file_name().to_string_lossy().contains("synccrate-restore")));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_damaged_index_blocks_gc_instead_of_losing_versions() {
        let root = crate::testutil::temp_dir("hist-bad");
        std::fs::create_dir_all(root.join(HISTORY_DIR)).unwrap();
        std::fs::write(root.join(HISTORY_DIR).join("sims4.json"), b"{broken").unwrap();
        assert!(referenced_hashes(&root).is_err());
        assert!(load(&root, "sims4").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_refuses_a_traversal_path_from_a_tampered_index() {
        let (root, _base, b) = setup();
        let mut h = HistoryFile { version: FORMAT_VERSION, entries: vec![] };
        let (hash, size, _) = backup::store_object(&root, &Path::new(&b).join("Mods/a.package")).unwrap();
        h.entries.push(FileVersion { path: "../../outside.txt".into(), ..v("x", &hash, size, 1) });
        save(&root, "sims4", &h).unwrap();
        let id = h.entries[0].id.clone();
        assert!(restore(&root, "sims4", &b, &id, 2).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
