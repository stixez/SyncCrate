use crate::commands::files::{get_game_def, resolve_game};
use crate::event_sink::{self, EventSink, Events};
use crate::registry::ContentType;
use crate::state::{AppState, SyncAction, SyncPlan};
use crate::utils;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_BACKUP_FILES: usize = 100_000;
const MAX_BACKUP_LABEL_LEN: usize = 128;
/// Manifests without a version (0) store each file under
/// `<id>/<category>/<rel>`; version 2 references objects in `objects/`.
const MANIFEST_VERSION: u32 = 2;
const OBJECTS_DIR: &str = "objects";
const SCHEDULE_FILE: &str = "schedule.json";
/// Safety backups taken before a restore that are kept per game.
const SAFETY_KEEP: usize = 3;
const SCHEDULE_TICK_SECS: u64 = 60;
/// A failed scheduled backup is retried after this long, not every tick.
const SCHEDULE_RETRY_SECS: u64 = 30 * 60;

pub const KIND_MANUAL: &str = "manual";
pub const KIND_AUTO: &str = "auto";
pub const KIND_PRESYNC: &str = "presync";
pub const KIND_SAFETY: &str = "safety";

/// Serializes everything that writes or deletes in the backup store. Objects
/// are shared between backups, so garbage collection must never run while a
/// backup that has stored objects but not yet written its manifest is running.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static RESTORING: AtomicBool = AtomicBool::new(false);

pub(crate) fn store_lock() -> std::sync::MutexGuard<'static, ()> {
    STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// True while a restore rewrites a game folder (syncs and scheduled backups wait).
/// For commands that change game files or share them: a restore rewrites
/// the folder, and hosting, installing, toggling or deleting in the middle
/// of it served friends a half-restored folder (or had the exact-restore
/// cleanup delete a file dropped in meanwhile).
pub fn refuse_during_restore() -> Result<(), String> {
    if restore_in_progress() {
        return Err("A restore is running. Wait for it to finish, then try again.".to_string());
    }
    Ok(())
}

pub fn restore_in_progress() -> bool {
    RESTORING.load(Ordering::SeqCst)
}

pub(crate) struct RestoringGuard;
impl Drop for RestoringGuard {
    fn drop(&mut self) {
        RESTORING.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: u64,
    pub label: String,
    pub file_count: usize,
    pub total_size: u64,
    // Legacy per-category counts (Sims-shaped); kept for older frontends and
    // old manifests. `category_counts` has every content type of the game.
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
    /// manual / auto (scheduled) / presync / safety. Empty in old manifests;
    /// `effective_kind` infers it.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub category_counts: HashMap<String, usize>,
    /// Bytes this backup added to the object store (the rest was shared with
    /// earlier backups). None for old full-copy backups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bytes: Option<u64>,
}

fn default_game() -> String {
    "sims4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    #[serde(default)]
    version: u32,
    info: BackupInfo,
    files: Vec<BackupFileEntry>,
}

/// Just the header, so listing backups doesn't build every file entry.
#[derive(Deserialize)]
struct ManifestHead {
    info: BackupInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupFileEntry {
    relative_path: String,
    size: u64,
    category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    /// Modification time (ms since the epoch). Restored onto the file so a
    /// restored mod doesn't look newest (keep-newer, outdated-script checks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mtime_ms: Option<i64>,
}

pub(crate) fn effective_kind(info: &BackupInfo) -> &str {
    if !info.kind.is_empty() {
        info.kind.as_str()
    } else if info.label.starts_with("Pre-restore safety backup") {
        KIND_SAFETY
    } else if info.auto && info.label.contains("Pre-sync") {
        KIND_PRESYNC
    } else if info.auto {
        KIND_AUTO
    } else {
        KIND_MANUAL
    }
}

// ---------------------------------------------------------------------------
// Sources

#[derive(Debug, Clone)]
struct SourceFile {
    category: String,
    /// Relative to the content type's folder, '/'-separated.
    rel: String,
    abs: PathBuf,
    size: u64,
    mtime_ms: Option<i64>,
}

pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let d = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

pub(crate) fn set_mtime(path: &Path, ms: i64) -> std::io::Result<()> {
    if ms < 0 {
        return Ok(());
    }
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
    std::fs::File::options().write(true).open(path)?.set_modified(t)
}

fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Comparable form of a path: `.` segments dropped, '/' separators, lowercase
/// (Windows paths are case-insensitive; on other systems this errs on the
/// side of keeping files).
fn norm_key(p: &Path) -> String {
    let pb: PathBuf = p.components().collect();
    pb.to_string_lossy().replace('\\', "/").to_lowercase()
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

/// Every file the game's content types cover (same rules as a scan — a
/// non-recursive "." type for loose presets must not grab the whole folder).
fn collect_full(base: &Path, cts: &[ContentType]) -> Result<Vec<SourceFile>, String> {
    let mut out = Vec::new();
    for ct in cts {
        let dir = base.join(&ct.folder);
        if !dir.is_dir() {
            continue;
        }
        for entry in content_walker(&dir, Some(ct)).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() || entry.path_is_symlink() {
                continue;
            }
            if !crate::commands::files::content_type_accepts(ct, entry.path()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&dir) else { continue };
            if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                continue;
            }
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            out.push(SourceFile {
                category: ct.id.clone(),
                rel: rel_string(rel),
                abs: entry.path().to_path_buf(),
                size: meta.len(),
                mtime_ms: mtime_ms(&meta),
            });
            if out.len() > MAX_BACKUP_FILES {
                return Err(format!("Too many files (>{MAX_BACKUP_FILES}). Aborting backup."));
            }
        }
    }
    Ok(out)
}

/// Only the given game-folder-relative files (a pre-sync backup of what the
/// sync will replace). Missing files and paths no content type covers are
/// skipped.
fn collect_targeted(base: &Path, cts: &[ContentType], paths: &[String]) -> Vec<SourceFile> {
    let base_str = base.to_string_lossy();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        if !seen.insert(p.to_lowercase()) {
            continue;
        }
        let Some((ct, rest)) = crate::sync::diff::content_type_for(cts, p) else { continue };
        let Ok(abs) = utils::safe_join(&base_str, p) else { continue };
        let Ok(meta) = std::fs::symlink_metadata(&abs) else { continue };
        if !meta.is_file() {
            continue;
        }
        out.push(SourceFile {
            category: ct.id.clone(),
            rel: rest,
            abs,
            size: meta.len(),
            mtime_ms: mtime_ms(&meta),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Object store

pub(crate) fn is_valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn object_path(root: &Path, hash: &str) -> PathBuf {
    root.join(OBJECTS_DIR).join(&hash[..2]).join(hash)
}

/// Copy `src` into the store while hashing it. Returns (hash, bytes, newly stored).
pub(crate) fn store_object(root: &Path, src: &Path) -> Result<(String, u64, bool), String> {
    let tmp_dir = root.join(OBJECTS_DIR).join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp = tmp_dir.join(format!("{}.tmp", Uuid::new_v4()));
    let copied = (|| -> std::io::Result<(String, u64)> {
        let mut input = std::fs::File::open(src)?;
        let mut out = std::fs::File::create(&tmp)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            total += n as u64;
        }
        out.flush()?;
        Ok((hex::encode(hasher.finalize()), total))
    })();
    let (hash, size) = match copied {
        Ok(v) => v,
        Err(e) => {
            // The handles are closed by now (Windows can't delete open files).
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{}: {}", src.display(), e));
        }
    };
    let dest = object_path(root, &hash);
    if dest.exists() {
        let _ = std::fs::remove_file(&tmp);
        return Ok((hash, size, false));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        if !dest.exists() {
            return Err(format!("{}: {}", src.display(), e));
        }
        return Ok((hash, size, false));
    }
    Ok((hash, size, true))
}

type ReuseMap = HashMap<(String, String), (u64, i64, String)>;

/// (category, rel) -> (size, mtime, hash) from the game's object-store
/// backups, newest first. A file whose size and mtime still match is reused
/// without reading it.
fn build_reuse_map(root: &Path, game: &str) -> ReuseMap {
    let mut manifests: Vec<BackupManifest> = read_manifests(root)
        .into_iter()
        .map(|(_, m)| m)
        .filter(|m| m.version >= MANIFEST_VERSION && m.info.game == game)
        .collect();
    manifests.sort_by(|a, b| b.info.created_at.cmp(&a.info.created_at));
    let mut map = ReuseMap::new();
    for m in manifests {
        for f in m.files {
            if let (Some(hash), Some(mtime)) = (f.hash, f.mtime_ms) {
                if is_valid_hash(&hash) {
                    map.entry((f.category, f.relative_path)).or_insert((f.size, mtime, hash));
                }
            }
        }
    }
    map
}

fn store_sources(
    root: &Path,
    sources: &[SourceFile],
    reuse: &ReuseMap,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<(Vec<BackupFileEntry>, u64), String> {
    let mut entries = Vec::with_capacity(sources.len());
    let mut new_bytes = 0u64;
    let total = sources.len();
    for (i, src) in sources.iter().enumerate() {
        let reused = reuse
            .get(&(src.category.clone(), src.rel.clone()))
            .filter(|(size, mtime, hash)| {
                *size == src.size && Some(*mtime) == src.mtime_ms && object_path(root, hash).is_file()
            })
            .map(|(size, _, hash)| (hash.clone(), *size));
        let (hash, size) = match reused {
            Some(v) => v,
            None => {
                let (hash, size, fresh) = store_object(root, &src.abs)?;
                if fresh {
                    new_bytes += size;
                }
                (hash, size)
            }
        };
        entries.push(BackupFileEntry {
            relative_path: src.rel.clone(),
            size,
            category: src.category.clone(),
            hash: Some(hash),
            mtime_ms: src.mtime_ms,
        });
        progress(i + 1, total, &src.rel);
    }
    Ok((entries, new_bytes))
}

fn write_manifest(dir: &Path, manifest: &BackupManifest) -> Result<(), String> {
    let data = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    let tmp = dir.join("manifest.json.tmp");
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("manifest.json")).map_err(|e| e.to_string())
}

fn read_manifest(dir: &Path) -> Result<BackupManifest, String> {
    let data = std::fs::read_to_string(dir.join("manifest.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

fn backup_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != OBJECTS_DIR && e.path().is_dir())
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect()
}

fn read_manifests(root: &Path) -> Vec<(PathBuf, BackupManifest)> {
    backup_dirs(root)
        .into_iter()
        .filter_map(|d| read_manifest(&d).ok().map(|m| (d, m)))
        .collect()
}

/// Every backup's info with `kind` filled in (old manifests don't have it).
fn read_infos(root: &Path) -> Vec<BackupInfo> {
    backup_dirs(root)
        .into_iter()
        .filter_map(|d| {
            let data = std::fs::read_to_string(d.join("manifest.json")).ok()?;
            let mut info = serde_json::from_str::<ManifestHead>(&data).ok()?.info;
            info.kind = effective_kind(&info).to_string();
            Some(info)
        })
        .collect()
}

struct BackupRequest<'a> {
    game: &'a str,
    base: &'a Path,
    cts: &'a [ContentType],
    label: String,
    kind: &'static str,
    /// Only these game-folder-relative files; None = everything.
    only: Option<&'a [String]>,
}

/// Create a backup in the store at `root`. Ok(None) when there is nothing to
/// back up (never write empty backups: pruning used to keep those and delete
/// the real ones). The caller holds the store lock.
fn create_backup_inner(
    root: &Path,
    req: BackupRequest,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<Option<BackupInfo>, String> {
    if !req.base.is_dir() {
        return Err(format!("Game folder not found: {}", req.base.display()));
    }
    let sources = match req.only {
        None => collect_full(req.base, req.cts)?,
        Some(paths) => collect_targeted(req.base, req.cts, paths),
    };
    if sources.is_empty() {
        return Ok(None);
    }
    let reuse = build_reuse_map(root, req.game);

    let id = Uuid::new_v4().to_string();
    let dir = root.join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (files, new_bytes) = match store_sources(root, &sources, &reuse, progress) {
        Ok(v) => v,
        Err(e) => {
            // Collect the objects already stored now: GC otherwise only runs
            // after a prune or delete, so on a full disk the space stayed
            // used. Callers hold the store lock.
            let _ = std::fs::remove_dir_all(&dir);
            gc_logged(root);
            return Err(e);
        }
    };

    let mut category_counts: HashMap<String, usize> = HashMap::new();
    if req.only.is_none() {
        // Zero entries matter: they record which folders a full backup covered
        // (exact restore only cleans those).
        for ct in req.cts {
            category_counts.insert(ct.id.clone(), 0);
        }
    }
    for f in &files {
        *category_counts.entry(f.category.clone()).or_insert(0) += 1;
    }
    let count = |k: &str| category_counts.get(k).copied().unwrap_or(0);
    let info = BackupInfo {
        id: id.clone(),
        created_at: utils::timestamp_now(),
        label: req.label,
        file_count: files.len(),
        total_size: files.iter().map(|f| f.size).sum(),
        mods_count: count("mods"),
        saves_count: count("saves"),
        tray_count: count("tray"),
        screenshots_count: count("screenshots"),
        game: req.game.to_string(),
        auto: req.kind == KIND_AUTO || req.kind == KIND_PRESYNC,
        kind: req.kind.to_string(),
        category_counts,
        new_bytes: Some(new_bytes),
    };
    let manifest = BackupManifest { version: MANIFEST_VERSION, info: info.clone(), files };
    if let Err(e) = write_manifest(&dir, &manifest) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(Some(info))
}

// ---------------------------------------------------------------------------
// Pruning and garbage collection

/// `created_at` is second-granularity, so several backups made within the
/// same second (e.g. "undo" doing three quick presync backups in a row, or
/// just a fast manual backup spree) tie on it — sorting by it alone would
/// then keep or prune an arbitrary one of them (filesystem enumeration
/// order), not necessarily the one actually created last. The manifest
/// file's own mtime breaks the tie with far finer resolution.
fn backup_order_key(root: &Path, info: &BackupInfo) -> (u64, std::time::SystemTime) {
    let mtime = std::fs::metadata(root.join(&info.id).join("manifest.json"))
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH);
    (info.created_at, mtime)
}

/// Ids to delete so at most `keep` non-empty backups of `kind` remain for
/// `game`. Empty backups (from old versions) are always dropped and never
/// count toward `keep`, so they can't push out real ones.
fn prune_candidates(root: &Path, infos: &[BackupInfo], game: &str, kind: &str, keep: usize, protect: Option<&str>) -> Vec<String> {
    let mut matching: Vec<&BackupInfo> = infos
        .iter()
        .filter(|b| b.game == game && effective_kind(b) == kind && Some(b.id.as_str()) != protect)
        .collect();
    matching.sort_by(|a, b| backup_order_key(root, b).cmp(&backup_order_key(root, a)));
    let keep = keep.max(1);
    let mut kept = 0;
    let mut out = Vec::new();
    for b in matching {
        if b.file_count == 0 {
            out.push(b.id.clone());
        } else if kept < keep {
            kept += 1;
        } else {
            out.push(b.id.clone());
        }
    }
    out
}

fn is_reserved_id(id: &str) -> bool {
    id.eq_ignore_ascii_case(OBJECTS_DIR) || id.eq_ignore_ascii_case(SCHEDULE_FILE)
}

fn remove_backup_dir(root: &Path, id: &str) -> Result<(), String> {
    utils::sanitize_id(id)?;
    if is_reserved_id(id) {
        return Err("Invalid backup id".into());
    }
    let backup_dir = root.join(id);
    if !backup_dir.is_dir() {
        return Err("Backup not found".into());
    }
    let canonical = std::fs::canonicalize(&backup_dir).map_err(|e| e.to_string())?;
    let root_canonical = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    if !canonical.starts_with(&root_canonical) || canonical == root_canonical {
        return Err("Invalid backup path".into());
    }
    std::fs::remove_dir_all(&backup_dir).map_err(|e| e.to_string())
}

/// Delete pruned backups. Returns how many were removed.
fn prune_kind(root: &Path, game: &str, kind: &str, keep: usize, protect: Option<&str>) -> usize {
    let infos = read_infos(root);
    let mut removed = 0;
    for id in prune_candidates(root, &infos, game, kind, keep, protect) {
        match remove_backup_dir(root, &id) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("Failed to prune backup {}: {}", id, e),
        }
    }
    removed
}

/// Delete objects no manifest references. Aborts (deleting nothing) if any
/// manifest can't be read, since its objects would look unreferenced.
fn gc_objects(root: &Path) -> Result<usize, String> {
    let mut referenced: HashSet<String> = HashSet::new();
    for dir in backup_dirs(root) {
        let manifest = read_manifest(&dir)
            .map_err(|e| format!("GC skipped, unreadable manifest in {}: {}", dir.display(), e))?;
        referenced.extend(manifest.files.into_iter().filter_map(|f| f.hash));
    }
    // Per-file version history keeps objects of its own (`commands::history`).
    referenced.extend(
        crate::commands::history::referenced_hashes(root).map_err(|e| format!("GC skipped, unreadable file history: {}", e))?,
    );
    let objects = root.join(OBJECTS_DIR);
    // Temp files only exist while a backup holds the store lock (so do we).
    let _ = std::fs::remove_dir_all(objects.join("tmp"));
    let Ok(prefixes) = std::fs::read_dir(&objects) else { return Ok(0) };
    let mut removed = 0;
    for prefix in prefixes.filter_map(|e| e.ok()) {
        let pdir = prefix.path();
        // Only real `ab/` prefix folders: a junction placed here (`objects/ab
        // -> C:\Somewhere`) would otherwise have its contents deleted.
        let pname = prefix.file_name().to_string_lossy().to_string();
        let is_real_dir = std::fs::symlink_metadata(&pdir).is_ok_and(|m| m.file_type().is_dir());
        if !is_real_dir || pname.len() != 2 || !pname.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(&pdir) {
            for f in files.filter_map(|e| e.ok()) {
                let name = f.file_name().to_string_lossy().to_string();
                if !is_valid_hash(&name) || !f.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                if !referenced.contains(&name) && std::fs::remove_file(f.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        let _ = std::fs::remove_dir(&pdir); // only succeeds when empty
    }
    Ok(removed)
}

pub(crate) fn gc_logged(root: &Path) {
    match gc_objects(root) {
        Ok(n) if n > 0 => log::info!("Backup store: removed {} unreferenced object(s)", n),
        Ok(_) => {}
        Err(e) => log::warn!("{}", e),
    }
}

// ---------------------------------------------------------------------------
// Restore

#[derive(Debug, Default, Serialize)]
pub struct RestoreResult {
    pub restored: usize,
    /// Already identical (same size and modification time), left alone.
    pub unchanged: usize,
    /// Files left alone because the user has since disabled/enabled them.
    pub skipped: Vec<String>,
    /// Entries whose stored data is missing or whose content type no longer exists.
    pub missing: usize,
    /// Files removed by an exact restore.
    pub removed: usize,
    /// Set when the restore stopped part-way; `restored` files were written.
    pub error: Option<String>,
    /// Label of the safety backup holding the files as they were before.
    pub safety_backup: Option<String>,
}

fn resolve_ct<'a>(cts: &'a [ContentType], category: &str) -> Option<&'a ContentType> {
    cts.iter()
        .find(|c| c.id == category)
        // Legacy category name from old manifests.
        .or_else(|| if category == "mods" { cts.first() } else { None })
}

/// The other state of a file: `x.package` <-> `x.package.disabled`, plus the
/// legacy `_Disabled/<rel>` location in the mods folder.
fn twin_paths(dest: &Path, dest_base: &Path, mods_dir: Option<&Path>, rel: &str) -> Vec<PathBuf> {
    let suffix = crate::commands::files::DISABLED_SUFFIX;
    let mut out = Vec::new();
    if let Some(name) = dest.file_name().map(|n| n.to_string_lossy().to_string()) {
        if name.to_lowercase().ends_with(suffix) {
            out.push(dest.with_file_name(&name[..name.len() - suffix.len()]));
        } else {
            out.push(dest.with_file_name(format!("{}{}", name, suffix)));
        }
    }
    if mods_dir == Some(dest_base) {
        out.push(dest_base.join("_Disabled").join(rel));
    }
    out
}

/// The other state of a backed-up file, if it exists now: restoring
/// `x.package` next to a disabled `x.package.disabled` (or `_Disabled/x`)
/// silently re-enabled a mod the user had turned off, as a duplicate; the
/// reverse for a backed-up `x.disabled` next to an enabled `x`.
pub(crate) fn disabled_twin(dest: &Path, dest_base: &Path, mods_dir: Option<&Path>, rel: &str) -> Option<String> {
    let twins = twin_paths(dest, dest_base, mods_dir, rel);
    if let Some(t) = twins.first().filter(|t| t.exists()) {
        return t.file_name().map(|n| n.to_string_lossy().to_string());
    }
    if twins.get(1).is_some_and(|t| t.exists()) {
        return Some(format!("_Disabled/{}", rel));
    }
    None
}

fn is_unchanged(dest: &Path, entry: &BackupFileEntry) -> bool {
    let Some(want) = entry.mtime_ms else { return false };
    match std::fs::symlink_metadata(dest) {
        Ok(m) => m.is_file() && m.len() == entry.size && mtime_ms(&m) == Some(want),
        Err(_) => false,
    }
}

/// Copy via a temp file so a failed copy never leaves a truncated file.
fn restore_file(source: &Path, dest: &Path, mtime: Option<i64>) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dest.with_file_name(format!("{}.synccrate-restore.tmp", name));
    let r = (|| {
        std::fs::copy(source, &tmp)?;
        if let Some(ms) = mtime {
            set_mtime(&tmp, ms)?;
        }
        std::fs::rename(&tmp, dest)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// Write a backup's files back into `base`. With `exact`, also delete files in
/// the backup's content folders that the backup doesn't have — only files a
/// scan would include, and only after every copy succeeded.
fn restore_inner(
    root: &Path,
    backup_id: &str,
    manifest: &BackupManifest,
    base: &Path,
    cts: &[ContentType],
    exact: bool,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> RestoreResult {
    let backup_dir = root.join(backup_id);
    let mods_dir = cts.first().map(|ct| base.join(&ct.folder));
    let mut result = RestoreResult::default();
    let mut keep: HashSet<String> = HashSet::new();
    let total = manifest.files.len();

    for (i, entry) in manifest.files.iter().enumerate() {
        progress(i + 1, total, &entry.relative_path);
        if !is_plain_relative_path(&entry.relative_path) {
            result.missing += 1;
            continue;
        }
        let Some(ct) = resolve_ct(cts, &entry.category) else {
            result.missing += 1;
            continue;
        };
        // Same check as every other write from outside the game folder: no
        // ADS ':' names, and no escaping through a junction placed in the
        // folder after the backup was made. (`dest` itself keeps the plain
        // spelling: exact restore compares it with scanned paths.)
        let under_base = if ct.folder == "." { entry.relative_path.clone() } else { format!("{}/{}", ct.folder, entry.relative_path) };
        if utils::safe_join(&base.to_string_lossy(), &under_base).is_err() {
            result.skipped.push(format!("{} (unsafe path)", entry.relative_path));
            continue;
        }
        let dest_base = base.join(&ct.folder);
        let dest = dest_base.join(&entry.relative_path);
        keep.insert(norm_key(&dest));
        for t in twin_paths(&dest, &dest_base, mods_dir.as_deref(), &entry.relative_path) {
            keep.insert(norm_key(&t));
        }

        if let Some(twin) = disabled_twin(&dest, &dest_base, mods_dir.as_deref(), &entry.relative_path) {
            result.skipped.push(format!("{} ({} exists)", entry.relative_path, twin));
            continue;
        }
        let source = if manifest.version >= MANIFEST_VERSION {
            match entry.hash.as_deref().filter(|h| is_valid_hash(h)) {
                Some(h) => object_path(root, h),
                None => {
                    result.missing += 1;
                    continue;
                }
            }
        } else {
            backup_dir.join(&entry.category).join(&entry.relative_path)
        };
        if !source.is_file() {
            result.missing += 1;
            continue;
        }
        if is_unchanged(&dest, entry) {
            result.unchanged += 1;
            continue;
        }
        if let Err(e) = restore_file(&source, &dest, entry.mtime_ms) {
            result.error = Some(format!("{}: {}", entry.relative_path, e));
            return result;
        }
        result.restored += 1;
    }

    if exact {
        let covered: HashSet<String> = if manifest.info.category_counts.is_empty() {
            manifest.files.iter().map(|f| f.category.clone()).collect()
        } else {
            manifest.info.category_counts.keys().cloned().collect()
        };
        let covered_ids: HashSet<String> =
            covered.iter().filter_map(|c| resolve_ct(cts, c)).map(|ct| ct.id.clone()).collect();
        let in_scope: Vec<ContentType> = cts.iter().filter(|c| covered_ids.contains(&c.id)).cloned().collect();
        match collect_full(base, &in_scope) {
            Ok(current) => {
                for f in current {
                    if keep.contains(&norm_key(&f.abs)) {
                        continue;
                    }
                    match std::fs::remove_file(&f.abs) {
                        Ok(()) => result.removed += 1,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            result.error = Some(format!("Couldn't remove {}: {}", f.rel, e));
                            return result;
                        }
                    }
                }
            }
            Err(e) => result.error = Some(format!("Couldn't list files to remove: {}", e)),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Undo last sync

/// Whether a restore may start; also blocks a second undo (and a restore
/// blocks undo) since both use this one flag. `None` if one is already running.
pub(crate) fn try_begin_restoring() -> Option<RestoringGuard> {
    if RESTORING.swap(true, Ordering::SeqCst) {
        None
    } else {
        Some(RestoringGuard)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct UndoResult {
    /// Replaced or deleted files put back from the presync backup (includes
    /// ones already matching, so restoring twice is harmless).
    pub restored: usize,
    /// Added files (including "keep both" `_remote` copies) deleted.
    pub removed: usize,
    /// Left alone, with why: changed or recreated since the sync, or no
    /// backup available for that file.
    pub skipped: Vec<String>,
    /// The restore stopped part-way (e.g. disk full). The undo record is kept
    /// so it can be retried (and the presync backup stays protected).
    pub interrupted: bool,
}

/// Whether the file at `base`/`rel` is exactly what a sync wrote: same size,
/// mtime and content. Used to refuse touching a file the user has since
/// changed (or the sync never actually wrote, e.g. it failed mid-sync).
fn file_matches(base: &str, rel: &crate::commands::undo::RecordedFile) -> bool {
    let Ok(abs) = utils::safe_join(base, &rel.relative_path) else { return false };
    let Ok(meta) = std::fs::symlink_metadata(&abs) else { return false };
    meta.is_file()
        && meta.len() == rel.size
        && mtime_ms(&meta) == Some(rel.mtime_ms)
        && crate::commands::files::compute_file_hash(&abs).ok().as_deref() == Some(rel.hash.as_str())
}

/// The game-folder-relative path a presync backup entry was collected from
/// (the reverse of `content_type_for`/`collect_targeted`): `ct.folder` + `/` +
/// `entry.relative_path`, or just the latter for a `.`-folder content type.
/// Every file a backup holds, as (game-folder-relative path, object hash,
/// size, mtime), so file history can index a presync backup's objects
/// without copying the files again. The caller holds the store lock.
pub(crate) fn backup_objects(root: &Path, backup_id: &str, cts: &[ContentType]) -> Result<Vec<(String, String, u64, Option<i64>)>, String> {
    utils::sanitize_id(backup_id)?;
    let manifest = read_manifest(&root.join(backup_id))?;
    Ok(manifest
        .files
        .iter()
        .filter_map(|e| {
            let hash = e.hash.clone().filter(|h| is_valid_hash(h))?;
            object_path(root, &hash).is_file().then_some(())?;
            Some((entry_game_root_path(cts, e)?, hash, e.size, e.mtime_ms))
        })
        .collect())
}

fn entry_game_root_path(cts: &[ContentType], entry: &BackupFileEntry) -> Option<String> {
    let ct = resolve_ct(cts, &entry.category)?;
    let folder = ct.folder.replace('\\', "/");
    let folder = folder.trim_end_matches('/').trim_start_matches("./");
    Some(if folder.is_empty() || folder == "." {
        entry.relative_path.clone()
    } else {
        format!("{}/{}", folder, entry.relative_path)
    })
}

/// Undo one sync: delete the files it added (only the ones still exactly as
/// it left them), and restore the files it replaced or deleted from the
/// presync backup it points to (only the ones the user hasn't touched
/// since). Never overwrites a file that doesn't match. The caller holds the
/// restoring guard.
pub(crate) fn undo_apply(
    record: &crate::commands::undo::SyncRecord,
    base: &Path,
    cts: &[ContentType],
) -> UndoResult {
    let mut result = UndoResult::default();
    let base_str = base.to_string_lossy().to_string();
    let _lock = store_lock();

    for f in &record.added {
        let Ok(abs) = utils::safe_join(&base_str, &f.relative_path) else {
            result.skipped.push(format!("{} (invalid path)", f.relative_path));
            continue;
        };
        if std::fs::symlink_metadata(&abs).is_err() {
            continue; // already gone
        }
        if !file_matches(&base_str, f) {
            result.skipped.push(format!("{} (changed since the sync)", f.relative_path));
            continue;
        }
        match std::fs::remove_file(&abs) {
            Ok(()) => result.removed += 1,
            Err(e) => result.skipped.push(format!("{}: {}", f.relative_path, e)),
        }
    }

    let Some(backup_id) = &record.presync_backup_id else {
        for f in &record.replaced {
            result.skipped.push(format!("{} (no backup was made for this sync)", f.relative_path));
        }
        for p in &record.deleted {
            result.skipped.push(format!("{} (no backup was made for this sync)", p));
        }
        return result;
    };

    let root = utils::backups_dir();
    let manifest = match read_manifest(&root.join(backup_id)) {
        Ok(m) => m,
        Err(_) => {
            for f in &record.replaced {
                result.skipped.push(format!("{} (the presync backup is gone)", f.relative_path));
            }
            for p in &record.deleted {
                result.skipped.push(format!("{} (the presync backup is gone)", p));
            }
            return result;
        }
    };

    let mut wanted: HashSet<String> = HashSet::new();
    for f in &record.replaced {
        if file_matches(&base_str, f) {
            wanted.insert(crate::sync::diff::match_key(&f.relative_path));
        } else {
            result.skipped.push(format!("{} (changed since the sync)", f.relative_path));
        }
    }
    for p in &record.deleted {
        let still_absent = utils::safe_join(&base_str, p).map_or(true, |abs| !abs.exists());
        if still_absent {
            wanted.insert(crate::sync::diff::match_key(p));
        } else {
            result.skipped.push(format!("{} (recreated since the sync)", p));
        }
    }

    if !wanted.is_empty() {
        let restricted = BackupManifest {
            version: manifest.version,
            info: manifest.info.clone(),
            files: manifest
                .files
                .iter()
                .filter(|e| {
                    entry_game_root_path(cts, e)
                        .is_some_and(|p| wanted.contains(&crate::sync::diff::match_key(&p)))
                })
                .cloned()
                .collect(),
        };
        let r = restore_inner(&root, backup_id, &restricted, base, cts, false, &mut |_, _, _| {});
        result.restored += r.restored + r.unchanged;
        result.skipped.extend(r.skipped);
        if r.missing > 0 {
            result.skipped.push(format!("{} file(s) missing from the backup", r.missing));
        }
        if let Some(e) = r.error {
            result.skipped.push(format!("restore stopped: {}", e));
            result.interrupted = true;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Pre-sync backup

/// Game-folder-relative local files the plan will overwrite ("use theirs"
/// downloads) or delete. Plain receives never replace an existing file, so a
/// plan of only new files needs no backup.
pub(crate) fn presync_targets(plan: &SyncPlan) -> Vec<String> {
    let excluded: std::collections::HashSet<&str> = plan.excluded.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for action in &plan.actions {
        match action {
            SyncAction::ReceiveFromRemote(f) => {
                if excluded.contains(f.relative_path.as_str()) {
                    continue;
                }
                if let Some(target) = plan.use_theirs.get(&f.relative_path) {
                    out.push(target.local_path.clone());
                }
            }
            SyncAction::Delete(p) => {
                if !excluded.contains(p.as_str()) {
                    out.push(p.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// Back up the files a sync is about to replace. Ok(None) if none of them
/// exist locally.
pub async fn create_presync_backup(
    events: &Events,
    game: String,
    base: String,
    cts: Vec<ContentType>,
    targets: Vec<String>,
) -> Result<Option<BackupInfo>, String> {
    let max_count = crate::commands::sync::read_sync_config().auto_backup_max_count as usize;
    let mut progress = progress_emitter(events.clone(), "backup-progress", KIND_PRESYNC, game.clone());
    tokio::task::spawn_blocking(move || {
        let _lock = store_lock();
        let root = utils::backups_dir();
        let label = format!("Before sync {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
        let info = create_backup_inner(
            &root,
            BackupRequest { game: &game, base: Path::new(&base), cts: &cts, label, kind: KIND_PRESYNC, only: Some(targets.as_slice()) },
            &mut progress,
        )?;
        if info.is_some() {
            // Never prune the presync backup a live undo record still points
            // to, even if it's fallen out of the newest-`max_count` window.
            let protect = crate::commands::undo::protected_presync_id(&game);
            if prune_kind(&root, &game, KIND_PRESYNC, max_count, protect.as_deref()) > 0 {
                gc_logged(&root);
            }
        }
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Progress

/// Emits progress at most every 100 ms (plus the last file): per-file events
/// for a 50k-file backup flooded the webview.
fn progress_emitter(
    app: Events,
    event: &'static str,
    phase: &'static str,
    game: String,
) -> impl FnMut(usize, usize, &str) + Send + 'static {
    let mut last: Option<std::time::Instant> = None;
    move |done, total, file| {
        let now = std::time::Instant::now();
        if done == total || last.map_or(true, |t| now.duration_since(t).as_millis() >= 100) {
            last = Some(now);
            app.emit(
                event,
                serde_json::json!({
                    "phase": phase,
                    "game": game,
                    "file": file,
                    "files_done": done,
                    "files_total": total,
                }),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Commands

fn validate_label(label: &str) -> Result<String, String> {
    let label = label.trim().to_string();
    if label.is_empty() || label.len() > MAX_BACKUP_LABEL_LEN {
        return Err(format!("Label must be 1-{MAX_BACKUP_LABEL_LEN} characters"));
    }
    if label.chars().any(|c| c.is_control()) {
        return Err("Label contains invalid characters".into());
    }
    Ok(label)
}

fn truncate_label(label: String) -> String {
    if label.len() <= MAX_BACKUP_LABEL_LEN {
        return label;
    }
    let mut end = MAX_BACKUP_LABEL_LEN - 3;
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &label[..end])
}

#[tauri::command]
pub async fn create_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    label: String,
    game: Option<String>,
) -> Result<BackupInfo, String> {
    let label = validate_label(&label)?;

    let workshop_app: Option<u32>;
    let (base, game_id, content_types, game_label) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let game_label = app_state.game_label(&game_id);
        workshop_app = app_state.game_registry.games.iter().find(|g| g.id == game_id).and_then(|g| g.steam_workshop_app_id);
        let path = app_state.game_paths.get(&game_id).cloned()
            .ok_or_else(|| format!("{} path not set", game_label))?;
        if !Path::new(&path).is_dir() {
            return Err(crate::commands::files::missing_folder_error(&game_label, &path));
        }
        let cts = get_game_def(&app_state.game_registry, &game_id)
            .map(|def| def.content_types.clone())
            .unwrap_or_default();
        (path, game_id, cts, game_label)
    };

    let mut progress = progress_emitter(event_sink::from_app(&app), "backup-progress", KIND_MANUAL, game_id.clone());
    let info = tokio::task::spawn_blocking(move || {
        let _lock = store_lock();
        create_backup_inner(
            &utils::backups_dir(),
            BackupRequest { game: &game_id, base: Path::new(&base), cts: &content_types, label, kind: KIND_MANUAL, only: None },
            &mut progress,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    info.ok_or_else(|| {
        // Workshop-installed mods (tModLoader) live in Steam's folder, not
        // the game's; say so instead of a bare "no files" (GitHub issue #2).
        let workshop = workshop_app
            .map(|id| crate::commands::files::count_workshop_items(&utils::steam_steamapps_dirs(), id))
            .unwrap_or(0);
        if workshop > 0 {
            format!(
                "Nothing to back up: no files found in {}'s folders. Your {} mods from the Steam Workshop are stored by Steam, outside the game folder, so SyncCrate can't back them up.",
                game_label, workshop
            )
        } else {
            format!("Nothing to back up: no files found in {}'s content folders.", game_label)
        }
    })
}

#[tauri::command]
pub async fn list_backups() -> Result<Vec<BackupInfo>, String> {
    let mut backups = tokio::task::spawn_blocking(|| read_infos(&utils::backups_dir()))
        .await
        .map_err(|e| e.to_string())?;
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(backups)
}

#[tauri::command]
pub async fn restore_backup(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    id: String,
    exact: Option<bool>,
) -> Result<RestoreResult, String> {
    utils::sanitize_id(&id)?;
    if is_reserved_id(&id) {
        return Err("Backup not found".into());
    }
    let exact = exact.unwrap_or(false);
    let root = utils::backups_dir();
    let backup_dir = root.join(&id);
    let manifest_path = backup_dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err("Backup not found".into());
    }
    let head = {
        let path = manifest_path.clone();
        tokio::task::spawn_blocking(move || -> Result<BackupInfo, String> {
            let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            Ok(serde_json::from_str::<ManifestHead>(&data).map_err(|e| e.to_string())?.info)
        })
        .await
        .map_err(|e| e.to_string())??
    };
    if exact && effective_kind(&head) == KIND_PRESYNC {
        return Err("Exact restore isn't available for a \"before sync\" backup: it only holds the files that sync replaced.".into());
    }

    let (base, game_id, content_types, process_names, game_label) = {
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
        let game_id = resolve_game(&app_state, &head.game)
            .map_err(|_| format!("Backup is for an unknown game '{}'", head.game))?;
        let game_label = app_state.game_label(&game_id);
        let path = app_state.game_paths.get(&game_id).cloned()
            .ok_or_else(|| format!("{} path not set. Configure it before restoring this backup.", game_label))?;
        if !Path::new(&path).is_dir() {
            return Err(crate::commands::files::missing_folder_error(&game_label, &path));
        }
        let def = get_game_def(&app_state.game_registry, &game_id);
        let cts = def.map(|d| d.content_types.clone()).unwrap_or_default();
        let procs = def.map(|d| d.process_names.clone()).unwrap_or_default();
        (path, game_id, cts, procs, game_label)
    };

    // A running game holds files open (Windows refuses to replace them) and
    // would keep using the old copies anyway.
    if crate::commands::game_state::is_game_running(&process_names).await.unwrap_or(false) {
        return Err(format!("{} is running. Close the game before restoring a backup.", game_label));
    }

    if RESTORING.swap(true, Ordering::SeqCst) {
        return Err("A restore is already running.".into());
    }
    let _guard = RestoringGuard;

    let events = event_sink::from_app(&app);
    let mut safety_progress = progress_emitter(events.clone(), "restore-progress", KIND_SAFETY, game_id.clone());
    let mut restore_progress = progress_emitter(events, "restore-progress", "restore", game_id.clone());
    tokio::task::spawn_blocking(move || -> Result<RestoreResult, String> {
        let _lock = store_lock();
        let manifest = read_manifest(&backup_dir)?;
        let base_path = Path::new(&base);

        // The safety backup is the only way back if the restore goes wrong, so a
        // failure here must abort the restore. (It used to be swallowed with
        // unwrap_or_default(), overwriting the user's files anyway.) Dedup makes
        // it cheap: unchanged files are shared with earlier backups.
        let safety_label = truncate_label(format!("Before restoring \"{}\"", manifest.info.label));
        let safety = create_backup_inner(
            &root,
            BackupRequest { game: &game_id, base: base_path, cts: &content_types, label: safety_label, kind: KIND_SAFETY, only: None },
            &mut safety_progress,
        )
        .map_err(|e| format!("Safety backup failed, restore aborted: {}", e))?;

        let mut result = restore_inner(&root, &id, &manifest, base_path, &content_types, exact, &mut restore_progress);
        result.safety_backup = safety.map(|s| s.label);

        // After restoring: pruning first could delete the very backup being
        // restored (an old safety backup) and GC its objects.
        if prune_kind(&root, &game_id, KIND_SAFETY, SAFETY_KEEP, Some(&id)) > 0 {
            gc_logged(&root);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rename_backup(id: String, label: String) -> Result<(), String> {
    utils::sanitize_id(&id)?;
    if is_reserved_id(&id) {
        return Err("Backup not found".into());
    }
    let label = validate_label(&label)?;
    tokio::task::spawn_blocking(move || {
        let _lock = store_lock();
        let backup_dir = utils::backups_dir().join(&id);
        if !backup_dir.join("manifest.json").is_file() {
            return Err("Backup not found".to_string());
        }
        let mut manifest = read_manifest(&backup_dir)?;
        manifest.info.label = label;
        write_manifest(&backup_dir, &manifest)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_backup(id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _lock = store_lock();
        let root = utils::backups_dir();
        remove_backup_dir(&root, &id)?;
        gc_logged(&root);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Scheduled backups

#[derive(Debug, Default, Serialize, Deserialize)]
struct Schedule {
    #[serde(default)]
    games: HashMap<String, GameSchedule>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
struct GameSchedule {
    /// Last successful (or "nothing to back up") run, unix seconds.
    #[serde(default)]
    last_backup_at: u64,
    #[serde(default)]
    last_attempt_at: u64,
}

fn load_schedule(root: &Path) -> Schedule {
    std::fs::read_to_string(root.join(SCHEDULE_FILE))
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default()
}

fn save_schedule(root: &Path, schedule: &Schedule) {
    if let Ok(data) = serde_json::to_string_pretty(schedule) {
        let _ = std::fs::write(root.join(SCHEDULE_FILE), data);
    }
}

/// Whether a game's scheduled backup is due. 0 = never. A timestamp in the
/// future (clock moved back) counts as due rather than blocking for days.
fn backup_due(now: u64, interval_secs: u64, last_backup: u64, last_attempt: u64) -> bool {
    let since = |t: u64| if t == 0 || t > now { u64::MAX } else { now - t };
    since(last_backup) >= interval_secs && since(last_attempt) >= SCHEDULE_RETRY_SECS.min(interval_secs)
}

/// When a game was last backed up per the schedule file, or (first run after
/// upgrading) its newest full backup, so a recent backup isn't repeated.
fn last_backup_for(schedule: &Schedule, infos: &[BackupInfo], game: &str) -> GameSchedule {
    if let Some(s) = schedule.games.get(game) {
        return *s;
    }
    let newest = infos
        .iter()
        .filter(|b| b.game == game && b.file_count > 0)
        .filter(|b| matches!(effective_kind(b), KIND_AUTO | KIND_MANUAL))
        .map(|b| b.created_at)
        .max()
        .unwrap_or(0);
    GameSchedule { last_backup_at: newest, last_attempt_at: 0 }
}

struct ScheduleCandidate {
    game: String,
    base: String,
    cts: Vec<ContentType>,
}

/// Checks every minute and backs up one due game per tick: each library game
/// with an available folder, on its own interval. (The old loop slept the full
/// interval from app start, so with restarts it almost never ran, and it only
/// covered the active game.)
pub fn spawn_scheduler(app: tauri::AppHandle, state: Arc<Mutex<AppState>>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(SCHEDULE_TICK_SECS)).await;
            let config = crate::commands::sync::read_sync_config();
            if !config.auto_backup_scheduled || restore_in_progress() {
                continue;
            }
            let candidates: Vec<ScheduleCandidate> = {
                let app_state = state.lock().await;
                if app_state.is_any_syncing() {
                    continue;
                }
                let mut games: Vec<String> = app_state.user_library.clone();
                if !games.contains(&app_state.active_game) {
                    games.insert(0, app_state.active_game.clone());
                }
                games
                    .into_iter()
                    .filter_map(|g| {
                        let base = app_state.game_paths.get(&g)?.clone();
                        let cts = get_game_def(&app_state.game_registry, &g)?.content_types.clone();
                        Some(ScheduleCandidate { game: g, base, cts })
                    })
                    .collect()
            };
            let interval = config.auto_backup_interval_hours.max(1) as u64 * 3600;
            let max_count = config.auto_backup_max_count as usize;
            let app2 = app.clone();
            let state2 = state.clone();
            let ran = tokio::task::spawn_blocking(move || {
                run_due_backup(&app2, &state2, candidates, interval, max_count)
            })
            .await
            .unwrap_or(false);
            if ran {
                app.emit("backups-changed", serde_json::Value::Null);
            }
        }
    });
}

/// Back up the first due candidate. Returns true if a backup was written.
fn run_due_backup(
    app: &tauri::AppHandle,
    state: &Arc<Mutex<AppState>>,
    candidates: Vec<ScheduleCandidate>,
    interval: u64,
    max_count: usize,
) -> bool {
    let root = utils::backups_dir();
    let now = utils::timestamp_now();
    let mut schedule = load_schedule(&root);
    let infos = read_infos(&root);
    let Some(c) = candidates.into_iter().find(|c| {
        let s = last_backup_for(&schedule, &infos, &c.game);
        Path::new(&c.base).is_dir() && backup_due(now, interval, s.last_backup_at, s.last_attempt_at)
    }) else {
        return false;
    };

    let _lock = store_lock();
    // A sync or restore may have started while we waited for the lock.
    if restore_in_progress() || state.try_lock().map_or(true, |s| s.is_any_syncing()) {
        return false;
    }
    let label = format!("Scheduled {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
    let mut progress = progress_emitter(event_sink::from_app(app), "backup-progress", KIND_AUTO, c.game.clone());
    let result = create_backup_inner(
        &root,
        BackupRequest { game: &c.game, base: Path::new(&c.base), cts: &c.cts, label, kind: KIND_AUTO, only: None },
        &mut progress,
    );
    let entry = schedule.games.entry(c.game.clone()).or_default();
    entry.last_attempt_at = now;
    let wrote = match result {
        Ok(Some(_)) => {
            entry.last_backup_at = now;
            log::info!("Scheduled backup of {} created", c.game);
            true
        }
        Ok(None) => {
            // Nothing to back up; try again next interval.
            entry.last_backup_at = now;
            false
        }
        Err(e) => {
            log::warn!("Scheduled backup of {} failed: {}", c.game, e);
            // Only logged before: the user believed backups were running
            // while one locked file failed every attempt.
            let _ = app.emit("backup-failed", serde_json::json!({ "game": &c.game, "error": &e }));
            false
        }
    };
    save_schedule(&root, &schedule);
    if wrote && prune_kind(&root, &c.game, KIND_AUTO, max_count, None) > 0 {
        gc_logged(&root);
    }
    wrote
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FileInfo, ReplaceTarget};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("synccrate-{}-{}", name, Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ct(id: &str, folder: &str, exts: &[&str]) -> ContentType {
        serde_json::from_value(serde_json::json!({
            "id": id, "label": id, "folder": folder, "file_type": "Mod", "extensions": exts,
        }))
        .unwrap()
    }

    fn write(path: &Path, data: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    fn backup(root: &Path, base: &Path, cts: &[ContentType], kind: &'static str, only: Option<&[String]>) -> Option<BackupInfo> {
        create_backup_inner(
            root,
            BackupRequest { game: "g", base, cts, label: "t".into(), kind, only },
            &mut |_, _, _| {},
        )
        .unwrap()
    }

    fn object_count(root: &Path) -> usize {
        WalkDir::new(root.join(OBJECTS_DIR)).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).count()
    }

    fn info(id: &str, kind: &str, created_at: u64, file_count: usize) -> BackupInfo {
        BackupInfo {
            id: id.into(), created_at, label: id.into(), file_count, total_size: 0, mods_count: 0, saves_count: 0,
            tray_count: 0, screenshots_count: 0, game: "g".into(), auto: kind == KIND_AUTO, kind: kind.into(),
            category_counts: HashMap::new(), new_bytes: None,
        }
    }

    #[test]
    fn object_store_dedups_and_reuses_by_size_and_mtime() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &["package"])];
        write(&base.join("Mods/a.package"), b"aaaa");
        write(&base.join("Mods/sub/b.package"), b"aaaa"); // same content as a
        write(&base.join("Mods/ignored.txt"), b"x");

        let first = backup(&root, &base, &cts, KIND_MANUAL, None).unwrap();
        assert_eq!(first.file_count, 2);
        assert_eq!(first.category_counts.get("mods"), Some(&2));
        assert_eq!(first.new_bytes, Some(4), "identical files are stored once");
        assert_eq!(object_count(&root), 1);

        // Unchanged files are reused from the manifest without reading them:
        // break the source's content but keep size+mtime to prove no read.
        let meta = std::fs::metadata(base.join("Mods/a.package")).unwrap();
        std::fs::write(base.join("Mods/a.package"), b"zzzz").unwrap();
        set_mtime(&base.join("Mods/a.package"), mtime_ms(&meta).unwrap()).unwrap();
        let second = backup(&root, &base, &cts, KIND_AUTO, None).unwrap();
        assert_eq!(second.new_bytes, Some(0));
        assert_eq!(object_count(&root), 1);

        // A changed file (new mtime) is read and stored.
        write(&base.join("Mods/sub/b.package"), b"bbbbbb");
        set_mtime(&base.join("Mods/sub/b.package"), 1_000_000).unwrap();
        let third = backup(&root, &base, &cts, KIND_AUTO, None).unwrap();
        assert_eq!(third.new_bytes, Some(6));
        assert_eq!(object_count(&root), 2);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gc_removes_only_unreferenced_objects() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &[])];
        write(&base.join("Mods/a.package"), b"one");
        let first = backup(&root, &base, &cts, KIND_MANUAL, None).unwrap();
        write(&base.join("Mods/b.package"), b"two");
        let second = backup(&root, &base, &cts, KIND_MANUAL, None).unwrap();
        assert_eq!(object_count(&root), 2);

        remove_backup_dir(&root, &second.id).unwrap();
        assert_eq!(gc_objects(&root).unwrap(), 1);
        assert_eq!(object_count(&root), 1, "a.package is still referenced by the first backup");

        // An unreadable manifest stops GC instead of deleting its objects.
        let broken = root.join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("manifest.json"), b"{not json").unwrap();
        remove_backup_dir(&root, &first.id).unwrap();
        assert!(gc_objects(&root).is_err());
        assert_eq!(object_count(&root), 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reserved_names_cant_be_deleted() {
        let root = tmp("store");
        std::fs::create_dir_all(root.join(OBJECTS_DIR)).unwrap();
        assert!(remove_backup_dir(&root, "objects").is_err());
        assert!(remove_backup_dir(&root, "..").is_err());
        assert!(root.join(OBJECTS_DIR).is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restores_old_format_backups() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &[])];
        let id = "old-backup";
        write(&root.join(id).join("mods").join("CC").join("x.package"), b"old data");
        let manifest: BackupManifest = serde_json::from_value(serde_json::json!({
            "info": { "id": id, "created_at": 1, "label": "old", "file_count": 1, "total_size": 8,
                      "mods_count": 1, "saves_count": 0 },
            "files": [{ "relative_path": "CC/x.package", "size": 8, "category": "mods" }],
        }))
        .unwrap();
        assert_eq!(manifest.version, 0);
        assert_eq!(effective_kind(&manifest.info), KIND_MANUAL);
        let r = restore_inner(&root, id, &manifest, &base, &cts, false, &mut |_, _, _| {});
        assert_eq!((r.restored, r.missing, r.error), (1, 0, None));
        assert_eq!(std::fs::read(base.join("Mods/CC/x.package")).unwrap(), b"old data");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_preserves_modification_times() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &[])];
        let file = base.join("Mods/a.package");
        write(&file, b"original");
        set_mtime(&file, 1_600_000_000_000).unwrap();
        let info = backup(&root, &base, &cts, KIND_MANUAL, None).unwrap();

        write(&file, b"changed later");
        let manifest = read_manifest(&root.join(&info.id)).unwrap();
        let r = restore_inner(&root, &info.id, &manifest, &base, &cts, false, &mut |_, _, _| {});
        assert_eq!(r.restored, 1);
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
        assert_eq!(mtime_ms(&std::fs::metadata(&file).unwrap()), Some(1_600_000_000_000));

        // Restoring again finds it identical.
        let r = restore_inner(&root, &info.id, &manifest, &base, &cts, false, &mut |_, _, _| {});
        assert_eq!((r.restored, r.unchanged), (0, 1));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn empty_or_missing_sources_are_refused() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &["package"])];
        assert!(backup(&root, &base, &cts, KIND_AUTO, None).is_none(), "no content folder");
        write(&base.join("Mods/readme.txt"), b"x");
        assert!(backup(&root, &base, &cts, KIND_AUTO, None).is_none(), "nothing the content type covers");
        assert!(read_infos(&root).is_empty(), "no empty backup written");
        let missing = base.join("gone");
        assert!(create_backup_inner(
            &root,
            BackupRequest { game: "g", base: &missing, cts: &cts, label: "t".into(), kind: KIND_AUTO, only: None },
            &mut |_, _, _| {},
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn prune_never_counts_empty_backups() {
        // No real backup dirs exist for these ids, so the mtime tiebreak
        // always falls back to UNIX_EPOCH for all of them — moot here since
        // every created_at below is already distinct.
        let root = std::path::Path::new("/nonexistent-prune-test-root");
        let infos = vec![
            info("old-full", KIND_AUTO, 1, 10),
            info("empty1", KIND_AUTO, 2, 0),
            info("empty2", KIND_AUTO, 3, 0),
            info("manual", KIND_MANUAL, 4, 0),
            info("new-full", KIND_AUTO, 5, 10),
        ];
        let mut out = prune_candidates(root, &infos, "g", KIND_AUTO, 2, None);
        out.sort();
        assert_eq!(out, vec!["empty1", "empty2"], "both real backups survive; manual untouched");
        let out = prune_candidates(root, &infos, "g", KIND_AUTO, 1, None);
        assert!(out.contains(&"old-full".to_string()) && !out.contains(&"new-full".to_string()));
        // keep is at least 1, and the protected id is never pruned.
        assert!(!prune_candidates(root, &infos, "g", KIND_AUTO, 0, None).contains(&"new-full".to_string()));
        assert!(!prune_candidates(root, &infos, "g", KIND_AUTO, 1, Some("old-full")).contains(&"old-full".to_string()));
    }

    #[test]
    fn prune_breaks_same_second_ties_by_manifest_mtime() {
        // Three backups created within the same `created_at` second (very
        // real for a quick succession of presync backups): without a finer
        // tiebreak, sorting by created_at alone leaves "newest" to whatever
        // order the filesystem happens to enumerate them in.
        let root = tmp("prune-tie");
        for id in ["a", "b", "c"] {
            std::fs::create_dir_all(root.join(id)).unwrap();
            std::fs::write(root.join(id).join("manifest.json"), "{}").unwrap();
        }
        // Give each manifest a distinct, increasing mtime — "c" is the real
        // most-recent one — while created_at (seconds) ties all three.
        set_mtime(&root.join("a").join("manifest.json"), 1_000_000).unwrap();
        set_mtime(&root.join("b").join("manifest.json"), 1_000_001).unwrap();
        set_mtime(&root.join("c").join("manifest.json"), 1_000_002).unwrap();
        let infos = vec![info("a", KIND_AUTO, 5, 10), info("b", KIND_AUTO, 5, 10), info("c", KIND_AUTO, 5, 10)];

        let mut out = prune_candidates(&root, &infos, "g", KIND_AUTO, 1, None);
        out.sort();
        assert_eq!(out, vec!["a".to_string(), "b".to_string()], "only \"c\" (truly newest) should survive");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exact_restore_only_removes_in_scope_files() {
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &["package"]), ct("saves", "Saves", &["save"])];
        write(&base.join("Mods/keep.package"), b"k");
        write(&base.join("Mods/off.package"), b"o");
        write(&base.join("Saves/slot.save"), b"s");
        let info = backup(&root, &base, &cts, KIND_MANUAL, None).unwrap();
        let manifest = read_manifest(&root.join(&info.id)).unwrap();

        write(&base.join("Mods/new.package"), b"n");
        write(&base.join("Mods/notes.txt"), b"not a mod");
        write(&base.join("Other/x.package"), b"outside content folders");
        std::fs::rename(base.join("Mods/off.package"), base.join("Mods/off.package.disabled")).unwrap();

        let r = restore_inner(&root, &info.id, &manifest, &base, &cts, true, &mut |_, _, _| {});
        assert_eq!(r.error, None);
        assert_eq!(r.removed, 1);
        assert!(!base.join("Mods/new.package").exists());
        assert!(base.join("Mods/notes.txt").exists(), "not covered by the content type");
        assert!(base.join("Other/x.package").exists(), "outside the content folders");
        assert!(base.join("Mods/off.package.disabled").exists(), "disabled twin kept");
        assert!(!base.join("Mods/off.package").exists());
        assert_eq!(r.skipped.len(), 1);

        // Without exact, extra files stay.
        write(&base.join("Mods/new2.package"), b"n");
        let r = restore_inner(&root, &info.id, &manifest, &base, &cts, false, &mut |_, _, _| {});
        assert_eq!(r.removed, 0);
        assert!(base.join("Mods/new2.package").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    fn fi(path: &str) -> FileInfo {
        serde_json::from_value(serde_json::json!({
            "relative_path": path, "size": 1, "hash": "h", "modified": 0, "file_type": "Mod",
        }))
        .unwrap()
    }

    #[test]
    fn presync_targets_only_replaced_or_deleted_files() {
        let mut plan = SyncPlan::default();
        plan.actions = vec![
            SyncAction::ReceiveFromRemote(fi("Mods/new.package")),
            SyncAction::ReceiveFromRemote(fi("Mods/theirs.package")),
            SyncAction::ReceiveFromRemote(fi("Mods/excluded.package")),
            SyncAction::Delete("Mods/gone.package".into()),
        ];
        plan.use_theirs.insert(
            "Mods/theirs.package".into(),
            ReplaceTarget { local_path: "Mods/theirs.package.disabled".into(), local_hash: "x".into() },
        );
        plan.use_theirs.insert(
            "Mods/excluded.package".into(),
            ReplaceTarget { local_path: "Mods/excluded.package".into(), local_hash: "x".into() },
        );
        plan.excluded.push("Mods/excluded.package".into());
        assert_eq!(presync_targets(&plan), vec!["Mods/theirs.package.disabled", "Mods/gone.package"]);

        // A targeted backup holds only those files.
        let (root, base) = (tmp("store"), tmp("game"));
        let cts = vec![ct("mods", "Mods", &["package"])];
        write(&base.join("Mods/theirs.package.disabled"), b"t");
        write(&base.join("Mods/other.package"), b"o");
        let targets = presync_targets(&plan);
        let b = backup(&root, &base, &cts, KIND_PRESYNC, Some(targets.as_slice())).unwrap();
        assert_eq!(b.file_count, 1, "gone.package doesn't exist locally");
        assert_eq!(b.kind, KIND_PRESYNC);
        let none: Vec<String> = Vec::new();
        assert!(backup(&root, &base, &cts, KIND_PRESYNC, Some(none.as_slice())).is_none());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scheduler_due_check() {
        let h = 3600;
        assert!(backup_due(10 * h, 4 * h, 0, 0), "never backed up");
        assert!(!backup_due(10 * h, 4 * h, 8 * h, 0));
        assert!(backup_due(10 * h, 4 * h, 6 * h, 0));
        assert!(!backup_due(10 * h, 4 * h, 6 * h, 10 * h - 60), "failed a minute ago: wait");
        assert!(backup_due(10 * h, 4 * h, 6 * h, 9 * h), "retry after 30 min");
        assert!(backup_due(10 * h, 4 * h, 20 * h, 0), "clock moved back");

        let infos = vec![info("a", KIND_AUTO, 500, 3), info("p", KIND_PRESYNC, 900, 1), info("e", KIND_AUTO, 800, 0)];
        let s = last_backup_for(&Schedule::default(), &infos, "g");
        assert_eq!(s.last_backup_at, 500, "presync and empty backups don't count");
        let mut sched = Schedule::default();
        sched.games.insert("g".into(), GameSchedule { last_backup_at: 7, last_attempt_at: 8 });
        assert_eq!(last_backup_for(&sched, &infos, "g").last_attempt_at, 8);
    }

    #[test]
    fn restore_skips_files_the_user_disabled_or_enabled() {
        let base = tmp("restore");
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
