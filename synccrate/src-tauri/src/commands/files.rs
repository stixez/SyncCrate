use crate::registry::{self, GameDefinition};
use crate::state::{AppState, FileInfo, FileManifest};
use crate::utils;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use walkdir::WalkDir;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct HashCacheEntry {
    size: u64,
    mtime: u64,
    hash: String,
}

type HashCache = HashMap<String, HashCacheEntry>;

fn load_hash_cache() -> HashCache {
    let path = utils::hash_cache_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(cache) = serde_json::from_str(&data) {
                return cache;
            }
        }
    }
    HashMap::new()
}

fn save_hash_cache(cache: &HashCache) {
    let path = utils::hash_cache_path();
    if let Ok(data) = serde_json::to_string(cache) {
        // Atomic write via a unique temp file + rename so a concurrent scan (e.g. the
        // host warm-up scan racing an on-demand rehash) can't leave a half-written
        // cache. The unique suffix avoids two writers clobbering the same temp file.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = path.with_extension(format!("{}.tmp", unique));
        if std::fs::write(&tmp, &data).is_ok() {
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct GameConfig {
    /// Saved paths only. Auto-detected paths live in `AppState::game_paths`
    /// but are never written here: persisting them let a leftover folder
    /// found at startup permanently replace a user's custom path (e.g. on a
    /// drive that was unplugged for one launch).
    pub game_paths: HashMap<String, String>,
    pub active_game: Option<String>,
    #[serde(default)]
    pub user_library: Vec<String>,
    /// Games whose path the user picked themselves. Older configs also contain
    /// auto-detected paths; those are kept but don't count as install evidence.
    #[serde(default)]
    pub user_set_paths: Vec<String>,
    /// Library games hidden from the sidebar (GitHub issue #3): still
    /// configured (folder, backups), just not listed.
    #[serde(default)]
    pub hidden_games: Vec<String>,
}

pub fn load_game_config() -> GameConfig {
    let path = utils::game_config_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<GameConfig>(&data) {
                return config;
            }
        }
    }
    GameConfig::default()
}

/// The persisted paths, kept in memory so saving the active game or library
/// never has to reconstruct them from `AppState::game_paths` (which mixes in
/// auto-detected ones).
#[derive(Default)]
pub(crate) struct SavedPaths {
    pub paths: HashMap<String, String>,
    pub user_set: std::collections::HashSet<String>,
}

static SAVED_PATHS: std::sync::LazyLock<std::sync::Mutex<SavedPaths>> = std::sync::LazyLock::new(|| {
    let c = load_game_config();
    std::sync::Mutex::new(SavedPaths { paths: c.game_paths, user_set: c.user_set_paths.into_iter().collect() })
});

fn saved_paths_lock() -> std::sync::MutexGuard<'static, SavedPaths> {
    SAVED_PATHS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Replace the saved paths (startup, after resolving legacy ids).
pub(crate) fn set_saved_paths(saved: SavedPaths) {
    *saved_paths_lock() = saved;
}

/// Path the user picked for `game_id`, if any (not auto-detected).
pub(crate) fn user_set_path(game_id: &str) -> Option<String> {
    let s = saved_paths_lock();
    if s.user_set.contains(game_id) { s.paths.get(game_id).cloned() } else { None }
}

pub(crate) fn save_game_config(app_state: &AppState) {
    let config = {
        let saved = saved_paths_lock();
        let mut user_set: Vec<String> = saved.user_set.iter().cloned().collect();
        user_set.sort();
        GameConfig {
            game_paths: saved.paths.clone(),
            active_game: Some(app_state.active_game.clone()),
            user_library: app_state.user_library.clone(),
            user_set_paths: user_set,
            hidden_games: app_state.hidden_games.clone(),
        }
    };
    let path = utils::game_config_path();
    if let Ok(data) = serde_json::to_string_pretty(&config) {
        let _ = std::fs::write(&path, data);
    }
}

/// Error for a saved folder that's gone. Scanning it would produce an empty
/// manifest, which looks like "the host has no mods" to everyone else.
pub(crate) fn missing_folder_error(label: &str, path: &str) -> String {
    format!(
        "{} folder not found: {}. Is the drive disconnected? Reconnect it or pick the folder again in Settings.",
        label, path
    )
}

/// Prefix of the `set_game_path` error that asks the UI to confirm an
/// unfamiliar folder and retry with `force`.
pub(crate) const NEEDS_CONFIRMATION: &str = "NEEDS_CONFIRMATION:";

/// Resolve `game` and require it to be the active game. Toggle/delete/profile
/// commands act on the active game's folder; the UI could show another game
/// (e.g. `set_active_game` refused mid-session) and then act on the wrong one.
pub(crate) fn require_active(app_state: &AppState, game: &str) -> Result<String, String> {
    let game_id = resolve_game(app_state, game)?;
    if game_id != app_state.active_game {
        return Err(format!(
            "{} isn't the active game ({} is). Disconnect from the session to manage {}'s files.",
            app_state.game_label(&game_id),
            app_state.game_label(&app_state.active_game),
            app_state.game_label(&game_id)
        ));
    }
    Ok(game_id)
}

/// Resolve a game ID string, accepting both new registry IDs and legacy enum variant names.
pub(crate) fn resolve_game(app_state: &AppState, game_str: &str) -> Result<String, String> {
    let registry_map = registry::build_registry_map(&app_state.game_registry);
    let legacy_map = registry::build_legacy_map(&app_state.game_registry);
    registry::resolve_game_id(game_str, &registry_map, &legacy_map)
        .ok_or_else(|| format!("Unknown game: {}", game_str))
}

/// Get the GameDefinition for a game ID.
pub(crate) fn get_game_def<'a>(registry: &'a crate::registry::GameRegistry, game_id: &str) -> Option<&'a GameDefinition> {
    registry.games.iter().find(|g| g.id == game_id)
}

/// Suffix `toggle_mod` appends when a game disables mods by renaming.
pub(crate) const DISABLED_SUFFIX: &str = ".disabled";

/// Lower-cased extension used for filtering/classification. A file disabled by
/// renaming (`hair.package.disabled`) keeps being treated as its real type.
pub(crate) fn effective_extension(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    let name = name.strip_suffix(DISABLED_SUFFIX).unwrap_or(&name);
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string()
}

/// Per-content-type filters beyond the extension list.
pub(crate) struct ScanFilter<'a> {
    pub recursive: bool,
    pub must_contain: Option<&'a str>,
    pub exclude_files: &'a [String],
}

impl ScanFilter<'_> {
    pub(crate) fn from_ct(ct: &crate::registry::ContentType) -> ScanFilter<'_> {
        ScanFilter {
            recursive: ct.recursive,
            must_contain: ct.must_contain.as_deref(),
            exclude_files: &ct.exclude_files,
        }
    }

    fn accepts(&self, path: &std::path::Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if self.exclude_files.iter().any(|x| x.eq_ignore_ascii_case(name)) {
            return false;
        }
        match self.must_contain {
            Some(needle) => file_head_contains(path, needle),
            None => true,
        }
    }
}

/// Whether a file under a content type's folder belongs to that content type
/// (extension list incl. `.disabled` files, exclusions, content sniffing).
/// Depth is enforced by the caller's walker (`ct.recursive`).
pub(crate) fn content_type_accepts(ct: &crate::registry::ContentType, path: &std::path::Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()).is_some_and(crate::sync::diff::is_synccrate_temp) {
        return false;
    }
    if !ct.extensions.is_empty() {
        let ext = effective_extension(path);
        if !ct.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
            return false;
        }
    }
    ScanFilter::from_ct(ct).accepts(path)
}

/// True if the first 64 KB of the file contain `needle` (lossy UTF-8).
fn file_head_contains(path: &std::path::Path, needle: &str) -> bool {
    use std::io::Read;
    let mut buf = Vec::with_capacity(65536);
    match std::fs::File::open(path) {
        Ok(f) => {
            if f.take(65536).read_to_end(&mut buf).is_err() {
                return false;
            }
        }
        Err(_) => return false,
    }
    String::from_utf8_lossy(&buf).contains(needle)
}

fn scan_directory(
    base_path: &str,
    sub_dir: &str,
    file_type_fn: impl Fn(&str) -> String + Sync,
    valid_extensions: &[String],
    filter: &ScanFilter,
    compute_hashes: bool,
    hash_cache: &HashCache,
) -> HashMap<String, FileInfo> {
    // "." (only allowed for non-recursive content types) means the game folder itself.
    let dir = if sub_dir == "." || sub_dir.is_empty() {
        std::path::PathBuf::from(base_path)
    } else {
        std::path::PathBuf::from(base_path).join(sub_dir)
    };
    let mut files = HashMap::new();

    if !dir.exists() {
        return files;
    }

    let ext_refs: Vec<&str> = valid_extensions.iter().map(|s| s.as_str()).collect();

    // Collect eligible file entries first, then hash in parallel
    let mut walker = WalkDir::new(&dir).follow_links(false);
    if !filter.recursive {
        walker = walker.max_depth(1);
    }
    let entries: Vec<_> = walker
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            // `file_type()` comes with the directory listing (free on
            // Windows); `path().is_file()` was an extra syscall per file.
            if entry.path_is_symlink() || !entry.file_type().is_file() {
                return false;
            }
            // If extensions list is non-empty, filter by them
            if !ext_refs.is_empty() {
                let ext = effective_extension(entry.path());
                if !ext_refs.contains(&ext.as_str()) {
                    return false;
                }
            }
            filter.accepts(entry.path())
        })
        .collect();

    let results: Vec<_> = entries
        .par_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let ext = effective_extension(path);

            let relative = path
                .strip_prefix(base_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
                .replace('\\', "/");

            // WalkDir's metadata (cached from the listing on Windows) rather than
            // a fresh stat per file.
            let metadata = entry.metadata().ok()?;
            let file_size = metadata.len();

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let hash = if compute_hashes {
                let cache_key = path.to_string_lossy().replace('\\', "/");
                if let Some(cached) = hash_cache.get(&cache_key) {
                    if cached.size == file_size && cached.mtime == modified {
                        cached.hash.clone()
                    } else {
                        compute_file_hash(path).ok()?
                    }
                } else {
                    compute_file_hash(path).ok()?
                }
            } else {
                String::new()
            };

            let file_type = file_type_fn(&ext);

            Some((
                relative.clone(),
                FileInfo {
                    relative_path: relative,
                    size: file_size,
                    hash,
                    modified,
                    file_type,
                },
            ))
        })
        .collect();

    for (key, info) in results {
        files.insert(key, info);
    }

    files
}

pub(crate) fn compute_file_hash(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::with_capacity(131072, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 131072];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Scan files for the active game, optionally computing hashes.
/// This is the shared implementation used by both the Tauri command and internal callers.
pub async fn scan_files_inner(
    state: &Arc<Mutex<AppState>>,
    game: Option<String>,
    compute_hashes: bool,
) -> Result<FileManifest, String> {
    let (game_id, existing, game_def, label) = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let def = get_game_def(&app_state.game_registry, &game_id)
            .ok_or_else(|| format!("Game '{}' not found in registry", game_id))?
            .clone();
        let label = app_state.game_label(&game_id);
        (game_id.clone(), app_state.game_paths.get(&game_id).cloned(), def, label)
    };
    let (base_path, newly_detected) = match existing {
        Some(p) => {
            if !std::path::Path::new(&p).is_dir() {
                return Err(missing_folder_error(&label, &p));
            }
            (p, false)
        }
        None => {
            // Only adopt a detected folder with real install evidence: a
            // leftover `Documents/.../<Game>` from an uninstalled game used to
            // be adopted (and populated with Mods/ Saves/) just by viewing it.
            let def = game_def.clone();
            let detected = tokio::task::spawn_blocking(move || {
                let p = utils::detect_game_path_from_def(&def)?;
                let ctx = crate::game_install::InstallContext::build();
                crate::game_install::detect_installed(&def, &ctx, Some(&p), None).then_some(p)
            })
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{} path not found. Please set it manually.", label))?;
            let mut app_state = state.lock().await;
            let p = app_state.game_paths.entry(game_id.clone()).or_insert(detected).clone();
            (p, true)
        }
    };
    let scanned_base = base_path.clone();

    // A just-auto-detected path bypasses set_game_path, which is where the game's
    // expected content folders normally get created. Mirror that here so, e.g.,
    // ReShade's reshade-presets/ folder exists on first detection rather than only
    // when the user manually re-picks the path. Only for installed games (above).
    if newly_detected {
        if let Some(val) = &game_def.validation {
            for dir in &val.auto_create_dirs {
                let _ = std::fs::create_dir_all(std::path::Path::new(&base_path).join(dir));
            }
        }
    }

    let manifest = tokio::task::spawn_blocking(move || {
        let hash_cache = if compute_hashes { load_hash_cache() } else { HashMap::new() };

        let mut all_files = HashMap::new();

        // Data-driven scanning: iterate content types from registry
        for ct in &game_def.content_types {
            let classify = ct.classify_by_extension.clone();
            let default_ft = ct.file_type.clone();
            let exts = ct.extensions.clone();

            let files = scan_directory(
                &base_path,
                &ct.folder,
                move |ext| {
                    classify.get(ext).cloned().unwrap_or_else(|| default_ft.clone())
                },
                &exts,
                &ScanFilter::from_ct(ct),
                compute_hashes,
                &hash_cache,
            );
            all_files.extend(files);
        }

        // Update hash cache with current scan results. Keep entries that belong
        // to other games/folders — replacing the whole cache with just this
        // scan threw away every other game's cached hashes on each game switch.
        if compute_hashes {
            let base_prefix = format!(
                "{}/",
                base_path.replace('\\', "/").trim_end_matches('/')
            );
            let mut new_cache: HashCache = hash_cache
                .into_iter()
                .filter(|(k, _)| !k.starts_with(&base_prefix))
                .collect();
            for info in all_files.values() {
                if !info.hash.is_empty() {
                    let abs_path = std::path::PathBuf::from(&base_path)
                        .join(&info.relative_path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    new_cache.insert(abs_path, HashCacheEntry {
                        size: info.size,
                        mtime: info.modified,
                        hash: info.hash.clone(),
                    });
                }
            }
            save_hash_cache(&new_cache);
        }

        FileManifest {
            files: all_files,
            generated_at: utils::timestamp_now(),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    // `local_manifest` is what we sync against and serve to peers, and it is
    // always interpreted relative to the *active* game's path. Only store the
    // result if this scan was for the active game (and its path didn't change
    // mid-scan); scanning another game (e.g. browsing it in the library) must
    // not replace the active game's manifest.
    let mut manifest = manifest;
    let mut app_state = state.lock().await;
    if app_state.active_game == game_id
        && app_state.game_paths.get(&game_id) == Some(&scanned_base)
    {
        if !compute_hashes {
            // A quick scan (e.g. triggered by the file watcher) has no hashes.
            // Carry over known hashes for unchanged files so a quick rescan
            // doesn't wipe the hashes a sync plan / peer manifest relies on
            // (empty local hashes make every shared file look like a conflict).
            carry_over_hashes(&mut manifest, &app_state.local_manifest);
        }
        app_state.local_manifest = manifest.clone();
    }
    Ok(manifest)
}

/// Fill empty hashes in `new` from `old` for files whose size and mtime are unchanged.
fn carry_over_hashes(new: &mut FileManifest, old: &FileManifest) {
    for (path, info) in new.files.iter_mut() {
        if !info.hash.is_empty() {
            continue;
        }
        if let Some(prev) = old.files.get(path) {
            if !prev.hash.is_empty() && prev.size == info.size && prev.modified == info.modified {
                info.hash = prev.hash.clone();
            }
        }
    }
}

#[tauri::command]
pub async fn scan_files(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
    quick: Option<bool>,
) -> Result<FileManifest, String> {
    let compute_hashes = !quick.unwrap_or(false);
    scan_files_inner(&*state, game, compute_hashes).await
}

#[tauri::command]
pub async fn get_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: String,
) -> Result<String, String> {
    let app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    app_state
        .game_paths
        .get(&game_id)
        .cloned()
        .or_else(|| utils::detect_game_path_from_registry(&game_id, &app_state.game_registry))
        .ok_or_else(|| format!("{} path not found", app_state.game_label(&game_id)))
}

/// Climb out of a known subfolder the user picked (`Mods` -> the game folder,
/// `Interface/AddOns` -> 2 levels up).
fn apply_path_correction(pc: &crate::registry::PathCorrection, picked: &std::path::Path) -> std::path::PathBuf {
    let mut out = picked.to_path_buf();
    let Some(folder) = picked.file_name().and_then(|n| n.to_str()) else { return out };
    let levels = match pc.nested_corrections.get(folder) {
        Some(&n) => n,
        None if pc.known_subfolders.iter().any(|s| s.eq_ignore_ascii_case(folder)) => 1,
        None => 0,
    };
    for _ in 0..levels {
        if let Some(parent) = out.parent() {
            out = parent.to_path_buf();
        }
    }
    out
}

/// Games that may legitimately share a folder: add-on variants of one game
/// (Sims 4 ReShade and GShade both live in `The Sims 4/Game/Bin`).
fn games_may_share(a: &GameDefinition, b: &GameDefinition) -> bool {
    let addon = |g: &GameDefinition| g.detection.as_ref().is_some_and(|d| !d.require_any.is_empty());
    a.family == b.family && addon(a) && addon(b)
}

/// Refuse drive roots, the user's home/Documents/Desktop/Downloads, and a
/// folder that is, contains, or sits inside another game's folder (`others`:
/// label + path). Picking `Documents` for The Sims made every other game's
/// files part of its manifest.
fn check_game_folder(
    candidate: &std::path::Path,
    others: &[(String, std::path::PathBuf)],
    protected: &[(std::path::PathBuf, &'static str)],
) -> Result<(), String> {
    if let Some(reason) = utils::protected_folder_reason(candidate, protected) {
        return Err(format!(
            "{} is {}. Pick the game's own folder instead.",
            candidate.display(),
            reason
        ));
    }
    if let Some((label, p)) = others.iter().find(|(_, p)| utils::paths_overlap(candidate, p)) {
        return Err(format!(
            "This folder overlaps {}'s folder ({}). Each game needs its own folder.",
            label,
            p.display()
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn set_game_path(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    game: String,
    path: String,
    force: Option<bool>,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    let label = app_state.game_label(&game_id);
    if app_state.active_game == game_id && app_state.session_type != crate::state::SessionType::None {
        // Peers sync against this folder's manifest, like set_active_game.
        return Err(format!("Disconnect from the current session before changing {}'s folder.", label));
    }
    let game_def = get_game_def(&app_state.game_registry, &game_id)
        .ok_or_else(|| format!("Game '{}' not found in registry", game_id))?
        .clone();

    let game_dir = std::path::Path::new(&path);
    if !game_dir.is_dir() {
        return Err("Path does not exist".to_string());
    }

    let picked = utils::clean_path(
        std::fs::canonicalize(game_dir)
            .map_err(|e| format!("Cannot resolve path: {}", e))?,
    );

    let others: Vec<(String, std::path::PathBuf)> = app_state
        .game_paths
        .iter()
        .filter(|(id, _)| **id != game_id)
        .filter(|(id, _)| {
            get_game_def(&app_state.game_registry, id).is_none_or(|d| !games_may_share(&game_def, d))
        })
        .map(|(id, p)| {
            let p = std::fs::canonicalize(p).map(utils::clean_path).unwrap_or_else(|_| p.into());
            (app_state.game_label(id), p)
        })
        .collect();
    let protected = utils::protected_folders();

    // Auto-correct if the user selected a known subfolder, unless climbing
    // would land in a drive root / user folder / another game's folder
    // (e.g. a "Mods" folder directly inside Documents).
    let canonical = match &game_def.path_correction {
        Some(pc) => {
            let corrected = apply_path_correction(pc, &picked);
            if corrected != picked && check_game_folder(&corrected, &others, &protected).is_err() {
                picked
            } else {
                corrected
            }
        }
        None => picked,
    };
    check_game_folder(&canonical, &others, &protected)?;

    // A folder with none of the game's expected subfolders is probably the
    // wrong one: ask before creating Mods/ Saves/ in it.
    if let Some(val) = &game_def.validation {
        let looks_right = val.check_dirs.is_empty() || val.check_dirs.iter().any(|d| canonical.join(d).exists());
        if !looks_right {
            if !force.unwrap_or(false) {
                return Err(format!(
                    "{}This doesn't look like a {} folder (no {} inside). Use it anyway?",
                    NEEDS_CONFIRMATION,
                    label,
                    val.check_dirs.join(" / ")
                ));
            }
            for dir in &val.auto_create_dirs {
                std::fs::create_dir_all(canonical.join(dir))
                    .map_err(|e| format!("Cannot create {} folder: {}", dir, e))?;
            }
        }
    }

    let new_path = canonical.to_string_lossy().to_string();
    if app_state.active_game == game_id
        && app_state.game_paths.get(&game_id) != Some(&new_path)
    {
        // Manifest paths were relative to the old folder.
        app_state.local_manifest = FileManifest::default();
    }
    let is_active = app_state.active_game == game_id;
    {
        let mut saved = saved_paths_lock();
        saved.paths.insert(game_id.clone(), new_path.clone());
        saved.user_set.insert(game_id.clone());
    }
    app_state.game_paths.insert(game_id, new_path);
    save_game_config(&app_state);
    if is_active {
        crate::watcher::file_watcher::restart_for_active(&mut app_state, app);
    }
    Ok(())
}

/// Games whose configured folder doesn't exist right now (unplugged drive,
/// moved library). Their saved path is kept; the UI flags them.
#[tauri::command]
pub async fn get_unavailable_game_paths(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    let paths = state.lock().await.game_paths.clone();
    let mut ids: Vec<String> = paths
        .into_iter()
        .filter(|(_, p)| !std::path::Path::new(p).is_dir())
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    Ok(ids)
}

#[tauri::command]
pub async fn get_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let app_state = state.lock().await;
    Ok(app_state.active_game.clone())
}

#[tauri::command]
pub async fn set_active_game(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    game: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    let game_id = resolve_game(&app_state, &game)?;
    let changed = app_state.active_game != game_id;
    if changed && app_state.session_type != crate::state::SessionType::None {
        // Peers are syncing against this game's manifest/folder.
        return Err("Disconnect from the current session before switching games.".to_string());
    }
    if changed {
        // The manifest belongs to the previous game; don't keep serving/syncing
        // it against the new game's path until a rescan replaces it.
        app_state.local_manifest = FileManifest::default();
    }
    app_state.active_game = game_id;
    save_game_config(&app_state);
    if changed {
        crate::watcher::file_watcher::restart_for_active(&mut app_state, app);
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_mod(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
    relative_path: String,
    enabled: bool,
) -> Result<String, String> {
    let (game_id, base, first_content_folder, rename_method) = {
        let app_state = state.lock().await;
        let game_id = require_active(&app_state, &game_id)?;
        let base = app_state
            .game_paths
            .get(&game_id)
            .cloned()
            .ok_or("Game path not set")?;
        let def = get_game_def(&app_state.game_registry, &game_id);
        let method = def.and_then(|d| d.disable_method.as_deref());
        if method == Some("none") {
            return Err(disable_unsupported_message(&app_state.game_label(&game_id)));
        }
        let folder = def
            .and_then(|d| d.content_types.first())
            .map(|ct| ct.folder.clone())
            .unwrap_or_else(|| "Mods".to_string());
        (game_id, base, folder, method == Some("rename"))
    };

    match toggle_file(&base, &first_content_folder, &relative_path, enabled, rename_method)? {
        Some(new_rel) => {
            // Tags are keyed by path; without this a toggle silently dropped them.
            crate::commands::tags::move_tags(&game_id, &relative_path.replace('\\', "/"), &new_rel);
            // Keep the manifest current without a full rescan (the frontend
            // patches its copy the same way; the watcher rescans afterwards).
            let mut app_state = state.lock().await;
            if app_state.active_game == game_id {
                if let Some(mut info) = app_state.local_manifest.files.remove(&relative_path.replace('\\', "/")) {
                    info.relative_path = new_rel.clone();
                    app_state.local_manifest.files.insert(new_rel.clone(), info);
                }
            }
            Ok(new_rel)
        }
        // Already in the requested state; nothing to move.
        None => Ok(relative_path.replace('\\', "/")),
    }
}

/// Why a `disable_method: "none"` game can't disable single files. Shared
/// with "apply pack exactly" so both explain it the same way.
pub(crate) fn disable_unsupported_message(game_label: &str) -> String {
    format!(
        "{} loads every subfolder and its mods are folders, so single files can't be disabled safely. Move the mod's folder out of the game to disable it.",
        game_label
    )
}

/// The file move behind `toggle_mod`, shared with "apply pack exactly"
/// (`commands::pack_apply`) so both disable/enable the same way. Returns the
/// new relative path, or `None` when the file is already in the requested
/// state. Never overwrites: an existing target is an error. Tags are left to
/// the caller.
pub(crate) fn toggle_file(
    base: &str,
    first_content_folder: &str,
    relative_path: &str,
    enabled: bool,
    rename_method: bool,
) -> Result<Option<String>, String> {
    let full_path = utils::safe_join(base, relative_path)?;
    if !full_path.exists() {
        return Err("File not found".into());
    }

    let mods_dir = std::path::PathBuf::from(base).join(first_content_folder);
    // Also validates the file is inside the mods folder, and handles files left
    // in a legacy `_Disabled/` folder.
    let folder_dest = toggle_destination(&mods_dir, &full_path, enabled)?;
    let dest = if rename_method {
        rename_destination(&full_path, enabled, folder_dest)
    } else {
        folder_dest
    };
    let Some(dest) = dest else { return Ok(None) };

    if dest.exists() {
        return Err(if enabled {
            format!("A file with that name already exists in {}", first_content_folder)
        } else if rename_method {
            "A disabled copy of this file already exists".to_string()
        } else {
            "A file with that name already exists in _Disabled".to_string()
        });
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&full_path, &dest).map_err(|e| e.to_string())?;

    let base_canonical = utils::clean_path(std::fs::canonicalize(base).map_err(|e| e.to_string())?);
    let dest_clean = utils::clean_path(dest.clone());
    let new_rel = dest_clean
        .strip_prefix(&base_canonical)
        .unwrap_or(&dest_clean)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(Some(new_rel))
}

/// Destination for games that disable by renaming (`x.package` <->
/// `x.package.disabled`). The Sims 3/4 load .package files from nested
/// subfolders, so moving a mod into `Mods/_Disabled/` didn't disable it.
/// Enabling a file still sitting in a legacy `_Disabled/` folder moves it back
/// out (`legacy_enable_dest`) and drops any `.disabled` suffix.
fn rename_destination(
    full_path: &std::path::Path,
    enabled: bool,
    legacy_enable_dest: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let name = full_path.file_name()?.to_string_lossy().to_string();
    if enabled {
        let target = legacy_enable_dest.unwrap_or_else(|| full_path.to_path_buf());
        let target_name = target.file_name()?.to_string_lossy().to_string();
        if target_name.to_lowercase().ends_with(DISABLED_SUFFIX) {
            let stripped = &target_name[..target_name.len() - DISABLED_SUFFIX.len()];
            Some(target.with_file_name(stripped))
        } else if target != full_path {
            Some(target)
        } else {
            None
        }
    } else if name.to_lowercase().ends_with(DISABLED_SUFFIX) {
        None
    } else {
        Some(full_path.with_file_name(format!("{}{}", name, DISABLED_SUFFIX)))
    }
}

/// Work out where `toggle_mod` should move a file.
///
/// Keeps the file's subfolder structure under the mods folder:
/// `Mods/Creator/x.package` <-> `Mods/_Disabled/Creator/x.package`.
/// (The old code flattened everything into `_Disabled/` and restored it to the
/// mods root, losing the original folder and colliding same-named files.)
///
/// Returns `Some(dest)`, or `None` when the file is already
/// in the requested state. Errors if the file isn't inside the mods folder.
fn toggle_destination(
    mods_dir: &std::path::Path,
    full_path: &std::path::Path,
    enabled: bool,
) -> Result<Option<std::path::PathBuf>, String> {
    let mods_canonical = utils::clean_path(
        std::fs::canonicalize(mods_dir).map_err(|e| format!("Mods folder not found: {}", e))?,
    );
    let file_clean = utils::clean_path(full_path.to_path_buf());
    let rel_in_mods = file_clean
        .strip_prefix(&mods_canonical)
        .map_err(|_| "Only files in the mods folder can be enabled or disabled".to_string())?;

    let mut comps = rel_in_mods.components();
    let in_disabled = matches!(
        comps.next(),
        Some(std::path::Component::Normal(first)) if first.eq_ignore_ascii_case("_Disabled")
    );
    let rest: std::path::PathBuf = comps.collect();

    Ok(match (enabled, in_disabled) {
        (true, true) => Some(mods_canonical.join(&rest)),
        (false, false) => Some(mods_canonical.join("_Disabled").join(rel_in_mods)),
        _ => None,
    })
}

/// Only folders inside a game folder or SyncCrate's own data can be opened.
/// It used to hand any existing path to explorer, which runs a file it's
/// given (`...\calc.exe`) and connects to a UNC path (`\\attacker\share`,
/// leaking the Windows login hash).
pub(crate) fn openable_folder(path: &str, allowed_roots: &[std::path::PathBuf]) -> Result<std::path::PathBuf, String> {
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err("Network paths can't be opened from SyncCrate.".into());
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| "Path does not exist".to_string())?;
    if !canonical.is_dir() {
        return Err("Only folders can be opened.".into());
    }
    let inside = allowed_roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|root| canonical.starts_with(&root));
    if !inside {
        return Err("That folder isn't part of a game SyncCrate manages.".into());
    }
    Ok(canonical)
}

#[tauri::command]
pub async fn open_folder(state: tauri::State<'_, Arc<Mutex<AppState>>>, path: String) -> Result<(), String> {
    let mut roots: Vec<std::path::PathBuf> = state.lock().await.game_paths.values().map(std::path::PathBuf::from).collect();
    roots.push(utils::config_root().join("synccrate"));
    let dir = openable_folder(&path, &roots)?;
    let path = crate::utils::clean_path(dir).to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(utils::windows_system_exe("explorer.exe"))
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_game_paths(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<HashMap<String, Option<String>>, String> {
    let app_state = state.lock().await;
    let mut result = HashMap::new();
    for game in &app_state.game_registry.games {
        result.insert(game.id.clone(), app_state.game_paths.get(&game.id).cloned());
    }
    Ok(result)
}

// --- New commands for game registry and library ---

#[tauri::command]
pub async fn get_game_registry(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<registry::GameDefinition>, String> {
    let app_state = state.lock().await;
    Ok(app_state.game_registry.games.clone())
}

#[tauri::command]
pub async fn get_user_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    let app_state = state.lock().await;
    Ok(app_state.user_library.clone())
}

#[tauri::command]
pub async fn add_to_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    if !app_state.game_registry.games.iter().any(|g| g.id == game_id) {
        return Err(format!("Unknown game: {}", game_id));
    }
    if !app_state.user_library.contains(&game_id) {
        app_state.user_library.push(game_id);
        save_game_config(&app_state);
    }
    Ok(())
}

/// Workshop items (one folder per item id) under every Steam library's
/// `steamapps/workshop/content/<app_id>`.
pub(crate) fn count_workshop_items(steamapps_dirs: &[std::path::PathBuf], app_id: u32) -> usize {
    let mut ids = std::collections::HashSet::new();
    for dir in steamapps_dirs {
        let Ok(rd) = std::fs::read_dir(dir.join("workshop").join("content").join(app_id.to_string())) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                ids.insert(e.file_name());
            }
        }
    }
    ids.len()
}

/// How many of the game's mods are Steam Workshop subscriptions (0 when the
/// game has no Workshop-installed mods). Those live outside the game folder,
/// so the Content page explains why they aren't listed (GitHub issue #2).
#[tauri::command]
pub async fn get_workshop_mod_count(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<usize, String> {
    let app_id = {
        let s = state.lock().await;
        let id = resolve_game(&s, &game)?;
        s.game_registry.games.iter().find(|g| g.id == id).and_then(|g| g.steam_workshop_app_id)
    };
    let Some(app_id) = app_id else { return Ok(0) };
    tokio::task::spawn_blocking(move || count_workshop_items(&utils::steam_steamapps_dirs(), app_id))
        .await
        .map_err(|e| e.to_string())
}

/// Hide or show `id` in `hidden`, keeping it sorted and duplicate-free.
pub(crate) fn set_hidden(hidden: &mut Vec<String>, id: &str, hide: bool) {
    hidden.retain(|g| g != id);
    if hide {
        hidden.push(id.to_string());
        hidden.sort();
    }
}

#[tauri::command]
pub async fn get_hidden_games(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<String>, String> {
    Ok(state.lock().await.hidden_games.clone())
}

/// Hide a library game from the sidebar (or show it again). Its folder and
/// everything else stay configured.
#[tauri::command]
pub async fn set_game_hidden(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
    hidden: bool,
) -> Result<Vec<String>, String> {
    let mut app_state = state.lock().await;
    if !app_state.game_registry.games.iter().any(|g| g.id == game_id) {
        return Err(format!("Unknown game: {}", game_id));
    }
    set_hidden(&mut app_state.hidden_games, &game_id, hidden);
    save_game_config(&app_state);
    Ok(app_state.hidden_games.clone())
}

#[tauri::command]
pub async fn remove_from_library(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<(), String> {
    let mut app_state = state.lock().await;
    app_state.user_library.retain(|id| id != &game_id);
    save_game_config(&app_state);
    Ok(())
}

#[tauri::command]
pub async fn detect_installed_games(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<HashMap<String, String>, String> {
    let app_state = state.lock().await;
    let registry = app_state.game_registry.clone();
    drop(app_state);

    let result = tokio::task::spawn_blocking(move || {
        let mut detected = HashMap::new();
        for game in &registry.games {
            if game.auto_detect {
                if let Some(path) = utils::detect_game_path_from_def(game) {
                    detected.insert(game.id.clone(), path);
                }
            }
        }
        detected
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(result)
}

/// Ids of games actually installed on this PC (Steam manifest, uninstall
/// entry, Epic/GOG, markers; see `game_install`). Unlike
/// `detect_installed_games`, a leftover mods/saves folder alone doesn't count.
#[tauri::command]
pub async fn get_installed_games(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    let app_state = state.lock().await;
    let registry = app_state.game_registry.clone();
    let game_paths = app_state.game_paths.clone();
    drop(app_state);

    tokio::task::spawn_blocking(move || {
        let ctx = crate::game_install::InstallContext::build();
        registry
            .games
            .iter()
            .filter(|game| {
                let detected = if game.auto_detect { utils::detect_game_path_from_def(game) } else { None };
                // Only a path the user picked counts on its own (see
                // detect_installed). Older configs saved auto-detected paths
                // too, and those may be leftovers.
                let user_set = user_set_path(&game.id).filter(|p| game_paths.get(&game.id) == Some(p));
                crate::game_install::detect_installed(game, &ctx, detected.as_deref(), user_set.as_deref())
            })
            .map(|game| game.id.clone())
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

// --- Legacy `_Disabled/` cleanup (rename-disable games) ---

/// Game path + first content type (the mods folder) of a `disable_method:
/// "rename"` game, or `None` for other games / unset paths.
fn rename_game_mods(
    app_state: &AppState,
    game: Option<String>,
) -> Result<Option<(String, crate::registry::ContentType)>, String> {
    let game_id = match game {
        Some(ref g) => resolve_game(app_state, g)?,
        None => app_state.active_game.clone(),
    };
    let def = match get_game_def(&app_state.game_registry, &game_id) {
        Some(d) => d,
        None => return Ok(None),
    };
    if def.disable_method.as_deref() != Some("rename") {
        return Ok(None);
    }
    let (base, ct) = match (app_state.game_paths.get(&game_id), def.content_types.first()) {
        (Some(b), Some(ct)) => (b.clone(), ct.clone()),
        _ => return Ok(None),
    };
    Ok(Some((base, ct)))
}

/// Mod files left in `<mods>/_Disabled/` by versions that disabled by moving.
/// Never follows symlinks/junctions (a linked `_Disabled` is ignored entirely).
fn legacy_disabled_files(
    mods_dir: &std::path::Path,
    ct: &crate::registry::ContentType,
) -> Vec<std::path::PathBuf> {
    let disabled = mods_dir.join("_Disabled");
    match std::fs::symlink_metadata(&disabled) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return Vec::new(),
    }
    WalkDir::new(&disabled)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| content_type_accepts(ct, p))
        .collect()
}

#[derive(serde::Serialize, Debug, Default)]
pub struct LegacyMigrationResult {
    pub moved: usize,
    /// Relative paths (inside `_Disabled/`) left in place because the target existed.
    pub collisions: Vec<String>,
    pub errors: Vec<String>,
}

/// Move every mod in `<mods>/_Disabled/<rel>` to `<mods>/<rel>.disabled`
/// (subfolders kept), then remove directories under `_Disabled` that are now empty.
fn migrate_legacy_disabled_in(
    mods_dir: &std::path::Path,
    ct: &crate::registry::ContentType,
) -> LegacyMigrationResult {
    let mut result = LegacyMigrationResult::default();
    let disabled = mods_dir.join("_Disabled");
    let mods_str = mods_dir.to_string_lossy().to_string();
    for src in legacy_disabled_files(mods_dir, ct) {
        let rel = match src.strip_prefix(&disabled) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let target_rel = if rel.to_lowercase().ends_with(DISABLED_SUFFIX) {
            rel.clone()
        } else {
            format!("{}{}", rel, DISABLED_SUFFIX)
        };
        // safe_join also rejects a target resolving outside the mods folder
        // (e.g. through a symlinked subfolder).
        let dest = match utils::safe_join(&mods_str, &target_rel) {
            Ok(d) => d,
            Err(e) => {
                result.errors.push(format!("{}: {}", rel, e));
                continue;
            }
        };
        if std::fs::symlink_metadata(&dest).is_ok() {
            result.collisions.push(rel);
            continue;
        }
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                result.errors.push(format!("{}: {}", rel, e));
                continue;
            }
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => result.moved += 1,
            Err(e) => result.errors.push(format!("{}: {}", rel, e)),
        }
    }
    // Deepest first; remove_dir only succeeds on empty dirs, and symlinked
    // dirs are reported as symlinks (not dirs) so they are never touched.
    if std::fs::symlink_metadata(&disabled).map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false) {
        for entry in WalkDir::new(&disabled).follow_links(false).contents_first(true).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_dir() {
                let _ = std::fs::remove_dir(entry.path());
            }
        }
    }
    result
}

/// Number of mods still sitting in a legacy `_Disabled/` folder (which The
/// Sims loads anyway). 0 for games that don't disable by renaming.
#[tauri::command]
pub async fn count_legacy_disabled(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<usize, String> {
    let target = rename_game_mods(&*state.lock().await, game)?;
    let Some((base, ct)) = target else { return Ok(0) };
    tokio::task::spawn_blocking(move || {
        legacy_disabled_files(&std::path::Path::new(&base).join(&ct.folder), &ct).len()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn migrate_legacy_disabled(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<LegacyMigrationResult, String> {
    let target = {
        let app_state = state.lock().await;
        if app_state.is_any_syncing() {
            return Err("Cannot move mods while a sync is in progress".into());
        }
        rename_game_mods(&app_state, game)?
    };
    let Some((base, ct)) = target else { return Ok(LegacyMigrationResult::default()) };
    tokio::task::spawn_blocking(move || {
        let mods_dir = std::path::Path::new(&base).join(&ct.folder);
        let mods_dir = std::fs::canonicalize(&mods_dir)
            .map(utils::clean_path)
            .map_err(|e| format!("Mods folder not found: {}", e))?;
        Ok(migrate_legacy_disabled_in(&mods_dir, &ct))
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- Duplicate finder ---

#[derive(serde::Serialize, Debug, Clone)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    /// Sorted by path; the UI keeps the first by default.
    pub files: Vec<FileInfo>,
    /// Bytes that would be freed by keeping only one copy.
    pub wasted: u64,
}

/// Group files with identical content hash. Files without a hash and
/// zero-byte files are ignored. Sorted by wasted space, largest first.
pub(crate) fn group_duplicates(manifest: &FileManifest) -> Vec<DuplicateGroup> {
    let mut by_hash: HashMap<&str, Vec<&FileInfo>> = HashMap::new();
    for f in manifest.files.values() {
        if f.size == 0 || f.hash.is_empty() {
            continue;
        }
        by_hash.entry(f.hash.as_str()).or_default().push(f);
    }
    let mut groups: Vec<DuplicateGroup> = by_hash
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(hash, v)| {
            let mut files: Vec<FileInfo> = v.into_iter().cloned().collect();
            files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            let size = files[0].size;
            DuplicateGroup {
                hash: hash.to_string(),
                size,
                wasted: size * (files.len() as u64 - 1),
                files,
            }
        })
        .collect();
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then_with(|| a.hash.cmp(&b.hash)));
    groups
}

/// Only files of the first (mods) content type take part in duplicate
/// grouping: identical saves, settings or world region files at different
/// paths are expected and must never be offered for deletion.
pub(crate) fn duplicate_candidates(manifest: &FileManifest, mods: &crate::registry::ContentType) -> FileManifest {
    let folder = mods.folder.replace('\\', "/").trim_matches('/').to_lowercase();
    let files = manifest
        .files
        .iter()
        .filter(|(rel, _)| {
            let rel = rel.to_lowercase();
            let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if folder == "." || folder.is_empty() {
                dir.is_empty()
            } else if mods.recursive {
                rel.starts_with(&format!("{}/", folder))
            } else {
                dir == folder
            }
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    FileManifest { files, generated_at: manifest.generated_at }
}

/// Hashed scan of the game followed by duplicate grouping (mods folder only,
/// games with `duplicate_finder` only).
#[tauri::command]
pub async fn find_duplicates(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<Vec<DuplicateGroup>, String> {
    let mods = {
        let app_state = state.lock().await;
        let game_id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let def = get_game_def(&app_state.game_registry, &game_id)
            .ok_or_else(|| format!("Game '{}' not found in registry", game_id))?;
        if !def.duplicate_finder {
            return Err(format!(
                "The duplicate finder isn't available for {}: identical files in different folders are normal there.",
                def.label
            ));
        }
        def.content_types.first().cloned().ok_or("Game has no content types")?
    };
    let manifest = scan_files_inner(&*state, game, true).await?;
    Ok(group_duplicates(&duplicate_candidates(&manifest, &mods)))
}

#[derive(serde::Serialize, Debug, Default)]
pub struct DeleteResult {
    pub deleted: usize,
    pub errors: Vec<String>,
}

/// Delete files of the active game. Every path must resolve (via `safe_join`,
/// no symlinks) to a regular file inside one of the game's content folders
/// that the content type accepts. `keep` maps a duplicate being deleted to
/// the copy that stays: that copy is re-checked right before the delete.
#[tauri::command]
pub async fn delete_mod_files(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
    paths: Vec<String>,
    keep: Option<HashMap<String, String>>,
) -> Result<DeleteResult, String> {
    let (base, cts) = {
        let app_state = state.lock().await;
        if app_state.is_any_syncing() {
            return Err("Cannot delete files while a sync is in progress".into());
        }
        let game_id = require_active(&app_state, &game_id)?;
        let base = app_state.game_paths.get(&game_id).cloned().ok_or("Game path not set")?;
        let cts = get_game_def(&app_state.game_registry, &game_id)
            .map(|d| d.content_types.clone())
            .unwrap_or_default();
        (base, cts)
    };
    let keep = keep.unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let mut result = DeleteResult::default();
        for rel in paths {
            let res = match keep.get(&rel) {
                Some(kept) => verify_kept_duplicate(&base, &cts, &rel, kept)
                    .and_then(|()| delete_content_file(&base, &cts, &rel)),
                None => delete_content_file(&base, &cts, &rel),
            };
            match res {
                Ok(()) => result.deleted += 1,
                Err(e) => result.errors.push(format!("{}: {}", rel, e)),
            }
        }
        result
    })
    .await
    .map_err(|e| e.to_string())
}

/// Before deleting duplicate `rel`, make sure `kept` still exists as a
/// different file with identical content. The duplicate list can be minutes
/// old: the kept copy may have been moved, deleted or updated since, and
/// deleting the "extra" would then remove the only copy.
fn verify_kept_duplicate(
    base: &str,
    cts: &[crate::registry::ContentType],
    rel: &str,
    kept: &str,
) -> Result<(), String> {
    let doomed = content_file_path(base, cts, rel)?;
    let kept_path = content_file_path(base, cts, kept)
        .map_err(|e| format!("the copy to keep ({}) is gone: {}", kept, e))?;
    if utils::same_path(&doomed, &kept_path) {
        return Err("it is the copy you chose to keep".into());
    }
    let (a, b) = (compute_file_hash(&doomed)?, compute_file_hash(&kept_path)?);
    if a != b {
        return Err(format!("the copy to keep ({}) changed; scan for duplicates again", kept));
    }
    Ok(())
}

fn delete_content_file(
    base: &str,
    cts: &[crate::registry::ContentType],
    rel: &str,
) -> Result<(), String> {
    let full = content_file_path(base, cts, rel)?;
    std::fs::remove_file(&full).map_err(|e| e.to_string())
}

/// Canonical path of `rel` if it's a regular file (not a symlink) inside one
/// of the content folders and accepted by that content type.
fn content_file_path(
    base: &str,
    cts: &[crate::registry::ContentType],
    rel: &str,
) -> Result<std::path::PathBuf, String> {
    let full = utils::safe_join(base, rel)?;
    let meta = std::fs::symlink_metadata(&full).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Not a regular file".into());
    }
    let full_c = utils::clean_path(std::fs::canonicalize(&full).map_err(|e| e.to_string())?);
    let inside = cts.iter().any(|ct| {
        let Ok(folder) = std::fs::canonicalize(std::path::Path::new(base).join(&ct.folder)) else {
            return false;
        };
        let folder = utils::clean_path(folder);
        let in_folder = if ct.recursive {
            full_c.starts_with(&folder)
        } else {
            full_c.parent() == Some(folder.as_path())
        };
        in_folder && content_type_accepts(ct, &full_c)
    });
    if !inside {
        return Err("Only files in the game's content folders can be deleted".into());
    }
    Ok(full)
}

// --- "May be outdated after a game patch" ---

/// Unix time the game was last patched: mtime of its `version_detection.file`.
fn patch_time_of(def: &GameDefinition, base: &str) -> Option<u64> {
    let vd = def.version_detection.as_ref()?;
    let modified = std::fs::metadata(std::path::Path::new(base).join(&vd.file)).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Script mods (the game's `dangerous_script_extensions`) last modified
/// before the game patch time.
pub(crate) fn outdated_script_paths(
    manifest: &FileManifest,
    patch_time: u64,
    script_exts: &[String],
) -> Vec<String> {
    let mut paths: Vec<String> = manifest
        .files
        .values()
        .filter(|f| f.modified > 0 && f.modified < patch_time)
        .filter(|f| {
            let ext = effective_extension(std::path::Path::new(&f.relative_path));
            script_exts.iter().any(|e| e.eq_ignore_ascii_case(&ext))
        })
        .map(|f| f.relative_path.clone())
        .collect();
    paths.sort();
    paths
}

#[tauri::command]
pub async fn get_game_patch_time(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<Option<u64>, String> {
    let app_state = state.lock().await;
    let game_id = match game {
        Some(ref g) => resolve_game(&app_state, g)?,
        None => app_state.active_game.clone(),
    };
    let (Some(def), Some(base)) = (
        get_game_def(&app_state.game_registry, &game_id),
        app_state.game_paths.get(&game_id),
    ) else {
        return Ok(None);
    };
    Ok(patch_time_of(def, base))
}

#[derive(serde::Serialize, Debug, Default)]
pub struct OutdatedScripts {
    pub patch_time: Option<u64>,
    pub paths: Vec<String>,
}

/// Script mods of the active game older than its last patch (from the current
/// local manifest). Empty for games without `version_detection`.
#[tauri::command]
pub async fn get_outdated_scripts(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game_id: String,
) -> Result<OutdatedScripts, String> {
    let app_state = state.lock().await;
    // local_manifest belongs to the active game only.
    let game_id = require_active(&app_state, &game_id)?;
    let (Some(def), Some(base)) = (
        get_game_def(&app_state.game_registry, &game_id),
        app_state.game_paths.get(&game_id),
    ) else {
        return Ok(OutdatedScripts::default());
    };
    let Some(patch_time) = patch_time_of(def, base) else {
        return Ok(OutdatedScripts::default());
    };
    Ok(OutdatedScripts {
        patch_time: Some(patch_time),
        paths: outdated_script_paths(&app_state.local_manifest, patch_time, &def.dangerous_script_extensions),
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn open_folder_only_opens_managed_folders() {
        let root = std::env::temp_dir().join(format!("sc-open-{}", uuid::Uuid::new_v4()));
        let game = root.join("Game");
        std::fs::create_dir_all(game.join("Mods")).unwrap();
        std::fs::write(game.join("Mods").join("tool.exe"), b"MZ").unwrap();
        let other = root.join("Elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        let roots = vec![game.clone()];
        assert!(openable_folder(game.join("Mods").to_str().unwrap(), &roots).is_ok());
        assert!(openable_folder(game.join("Mods").join("tool.exe").to_str().unwrap(), &roots).unwrap_err().contains("folders"), "a file (would run it)");
        assert!(openable_folder(other.to_str().unwrap(), &roots).is_err(), "outside every game");
        assert!(openable_folder("\\\\attacker\\share", &roots).unwrap_err().contains("Network"));
        assert!(openable_folder(root.join("missing").to_str().unwrap(), &roots).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workshop_items_are_counted_once_across_libraries() {
        let a = crate::testutil::temp_dir("ws-a");
        let b = crate::testutil::temp_dir("ws-b");
        for (lib, id) in [(&a, "111"), (&a, "222"), (&b, "222"), (&b, "333")] {
            std::fs::create_dir_all(lib.join("workshop/content/1281930").join(id).join("2024.1")).unwrap();
        }
        std::fs::write(a.join("workshop/content/1281930/stray.txt"), b"x").unwrap();
        std::fs::create_dir_all(a.join("workshop/content/999/444")).unwrap();
        assert_eq!(count_workshop_items(&[a.clone(), b.clone(), a.join("missing")], 1281930), 3);
        assert_eq!(count_workshop_items(&[a.clone()], 5), 0);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn hidden_games_toggle_and_old_configs_parse() {
        let mut h = vec![];
        set_hidden(&mut h, "valheim", true);
        set_hidden(&mut h, "cs2", true);
        set_hidden(&mut h, "valheim", true);
        assert_eq!(h, vec!["cs2".to_string(), "valheim".to_string()], "sorted, no duplicates");
        set_hidden(&mut h, "valheim", false);
        assert_eq!(h, vec!["cs2".to_string()]);
        // A config from before hidden games existed still loads.
        let old: GameConfig = serde_json::from_str(r#"{"game_paths":{},"active_game":"sims4","user_library":["sims4"]}"#).unwrap();
        assert!(old.hidden_games.is_empty());
    }
    use super::*;

    fn mods_ct() -> crate::registry::ContentType {
        serde_json::from_value(serde_json::json!({
            "id": "mods", "label": "Mods", "folder": "Mods",
            "extensions": ["package", "ts4script"], "file_type": "Mod"
        }))
        .unwrap()
    }

    #[test]
    fn migrate_legacy_disabled_moves_files_and_reports_collisions() {
        let base = std::env::temp_dir().join(format!("synccrate-legacy-{}", uuid::Uuid::new_v4()));
        let mods = base.join("Mods");
        let dis = mods.join("_Disabled");
        std::fs::create_dir_all(dis.join("Creator").join("Deep")).unwrap();
        std::fs::create_dir_all(mods.join("Creator")).unwrap();
        std::fs::write(dis.join("root.package"), b"a").unwrap();
        std::fs::write(dis.join("Creator").join("Deep").join("hair.package"), b"b").unwrap();
        std::fs::write(dis.join("Creator").join("clash.package"), b"c").unwrap();
        std::fs::write(mods.join("Creator").join("clash.package.disabled"), b"old").unwrap();
        std::fs::write(dis.join("readme.txt"), b"not a mod").unwrap();

        let mods_c = utils::clean_path(std::fs::canonicalize(&mods).unwrap());
        let ct = mods_ct();
        assert_eq!(legacy_disabled_files(&mods_c, &ct).len(), 3);

        let r = migrate_legacy_disabled_in(&mods_c, &ct);
        assert_eq!(r.moved, 2);
        assert_eq!(r.collisions, vec!["Creator/clash.package".to_string()]);
        assert!(r.errors.is_empty());
        assert!(mods.join("root.package.disabled").is_file());
        assert!(mods.join("Creator").join("Deep").join("hair.package.disabled").is_file());
        // Emptied subfolder removed; folders still holding files kept.
        assert!(!dis.join("Creator").join("Deep").exists());
        assert!(dis.join("Creator").join("clash.package").is_file());
        assert!(dis.join("readme.txt").is_file());
        assert_eq!(legacy_disabled_files(&mods_c, &ct).len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn group_duplicates_ignores_empty_hashes_and_zero_bytes() {
        let m = manifest(vec![
            info("Mods/a.package", "h1", 100, 1),
            info("Mods/sub/a copy.package", "h1", 100, 2),
            info("Mods/b.package", "h2", 10, 1),
            info("Mods/b2.package", "h2", 10, 1),
            info("Mods/b3.package", "h2", 10, 1),
            info("Mods/unique.package", "h3", 5, 1),
            info("Mods/nohash1.package", "", 7, 1),
            info("Mods/nohash2.package", "", 7, 1),
            info("Mods/empty1.package", "e", 0, 1),
            info("Mods/empty2.package", "e", 0, 1),
        ]);
        let g = group_duplicates(&m);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].hash, "h1");
        assert_eq!(g[0].wasted, 100);
        assert_eq!(g[0].files[0].relative_path, "Mods/a.package");
        assert_eq!(g[1].files.len(), 3);
        assert_eq!(g[1].wasted, 20);
    }

    #[test]
    fn outdated_scripts_only_flags_old_script_files() {
        let m = manifest(vec![
            info("Mods/old.ts4script", "", 1, 100),
            info("Mods/old_disabled.TS4SCRIPT.disabled", "", 1, 100),
            info("Mods/new.ts4script", "", 1, 300),
            info("Mods/old.package", "", 1, 100),
            info("Mods/unknown_time.ts4script", "", 1, 0),
        ]);
        let exts = vec!["ts4script".to_string()];
        assert_eq!(
            outdated_script_paths(&m, 200, &exts),
            vec!["Mods/old.ts4script".to_string(), "Mods/old_disabled.TS4SCRIPT.disabled".to_string()]
        );
        assert!(outdated_script_paths(&m, 200, &[]).is_empty());
    }

    #[test]
    fn effective_extension_sees_through_disabled_suffix() {
        use std::path::Path;
        assert_eq!(effective_extension(Path::new("Mods/hair.package")), "package");
        assert_eq!(effective_extension(Path::new("Mods/Hair.PACKAGE.disabled")), "package");
        assert_eq!(effective_extension(Path::new("Mods/readme")), "");
    }

    #[test]
    fn rename_destination_toggles_suffix_and_migrates_legacy_folder() {
        use std::path::PathBuf;
        let f = PathBuf::from("Mods/CC/hair.package");
        assert_eq!(rename_destination(&f, false, None), Some(PathBuf::from("Mods/CC/hair.package.disabled")));
        assert_eq!(rename_destination(&f, true, None), None);

        let d = PathBuf::from("Mods/CC/hair.package.disabled");
        assert_eq!(rename_destination(&d, true, None), Some(PathBuf::from("Mods/CC/hair.package")));
        assert_eq!(rename_destination(&d, false, None), None);

        // Legacy `_Disabled/` file: enabling moves it back out of the folder.
        let legacy = PathBuf::from("Mods/_Disabled/CC/hair.package");
        let out = Some(PathBuf::from("Mods/CC/hair.package"));
        assert_eq!(rename_destination(&legacy, true, out.clone()), out);
    }

    #[test]
    fn content_type_accepts_applies_exclusions_and_sniffing() {
        let dir = std::env::temp_dir().join(format!("synccrate-ct-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let preset = dir.join("Cozy.ini");
        let config = dir.join("ReShade.ini");
        let other = dir.join("Game.ini");
        std::fs::write(&preset, "PreprocessorDefinitions=\nTechniques=Bloom@Bloom.fx\n").unwrap();
        std::fs::write(&config, "[GENERAL]\nTechniques=ignored-because-excluded\n").unwrap();
        std::fs::write(&other, "[Graphics]\nWidth=1920\n").unwrap();

        let ct: crate::registry::ContentType = serde_json::from_value(serde_json::json!({
            "id": "reshade_bin_presets", "label": "x", "folder": ".", "recursive": false,
            "extensions": ["ini"], "must_contain": "Techniques=",
            "exclude_files": ["ReShade.ini"], "file_type": "ReShadePreset"
        }))
        .unwrap();
        assert!(content_type_accepts(&ct, &preset));
        assert!(!content_type_accepts(&ct, &config));
        assert!(!content_type_accepts(&ct, &other));

        // The scanner only picks up the preset, keyed relative to the game folder.
        let files = scan_directory(
            &dir.to_string_lossy(),
            ".",
            |_| "ReShadePreset".to_string(),
            &ct.extensions,
            &ScanFilter::from_ct(&ct),
            false,
            &HashMap::new(),
        );
        let keys: Vec<&String> = files.keys().collect();
        assert_eq!(keys, vec!["Cozy.ini"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn info(path: &str, hash: &str, size: u64, modified: u64) -> FileInfo {
        FileInfo {
            relative_path: path.to_string(),
            size,
            hash: hash.to_string(),
            modified,
            file_type: "Mod".to_string(),
        }
    }

    fn manifest(files: Vec<FileInfo>) -> FileManifest {
        FileManifest {
            files: files.into_iter().map(|f| (f.relative_path.clone(), f)).collect(),
            generated_at: 0,
        }
    }

    #[test]
    fn quick_scan_keeps_hashes_of_unchanged_files_only() {
        let old = manifest(vec![
            info("Mods/same.package", "h1", 10, 100),
            info("Mods/changed.package", "h2", 10, 100),
        ]);
        let mut new = manifest(vec![
            info("Mods/same.package", "", 10, 100),
            info("Mods/changed.package", "", 11, 200),
            info("Mods/new.package", "", 5, 300),
        ]);
        carry_over_hashes(&mut new, &old);
        assert_eq!(new.files["Mods/same.package"].hash, "h1");
        assert_eq!(new.files["Mods/changed.package"].hash, "");
        assert_eq!(new.files["Mods/new.package"].hash, "");
    }

    fn temp_mods_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "synccrate_toggle_{}_{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Mods").join("Creator")).unwrap();
        dir
    }

    #[test]
    fn toggle_preserves_subfolders() {
        let base = temp_mods_dir("subfolders");
        let mods = base.join("Mods");
        let file = mods.join("Creator").join("hair.package");
        std::fs::write(&file, b"x").unwrap();

        let disabled = toggle_destination(&mods, &std::fs::canonicalize(&file).unwrap(), false)
            .unwrap()
            .expect("should move");
        assert!(disabled.ends_with("Mods/_Disabled/Creator/hair.package"));

        std::fs::create_dir_all(disabled.parent().unwrap()).unwrap();
        std::fs::rename(&file, &disabled).unwrap();
        let enabled = toggle_destination(&mods, &std::fs::canonicalize(&disabled).unwrap(), true)
            .unwrap()
            .expect("should move");
        assert!(enabled.ends_with("Mods/Creator/hair.package"));

        // Already disabled -> no move
        assert!(toggle_destination(&mods, &std::fs::canonicalize(&disabled).unwrap(), false)
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn toggle_rejects_files_outside_mods_folder() {
        let base = temp_mods_dir("outside");
        std::fs::create_dir_all(base.join("Saves")).unwrap();
        let save = base.join("Saves").join("slot.save");
        std::fs::write(&save, b"x").unwrap();
        let res = toggle_destination(&base.join("Mods"), &std::fs::canonicalize(&save).unwrap(), false);
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn duplicates_only_come_from_the_mods_folder() {
        let m = manifest(vec![
            info("Mods/a.package", "h1", 10, 1),
            info("Mods/CC/a copy.package", "h1", 10, 1),
            info("Saves/slot1.save", "h2", 10, 1),
            info("Saves/Backup/slot1.save", "h2", 10, 1),
            info("ModsExtra/x.package", "h1", 10, 1),
        ]);
        let cand = duplicate_candidates(&m, &mods_ct());
        let mut keys: Vec<&String> = cand.files.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["Mods/CC/a copy.package", "Mods/a.package"]);
        let g = group_duplicates(&cand);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].files.len(), 2);

        // Non-recursive first content type: only its direct files.
        let flat: crate::registry::ContentType = serde_json::from_value(serde_json::json!({
            "id": "mods", "label": "Mods", "folder": "Mods", "recursive": false, "file_type": "Mod"
        }))
        .unwrap();
        assert_eq!(duplicate_candidates(&m, &flat).files.len(), 1);
    }

    #[test]
    fn deleting_a_duplicate_rechecks_the_kept_copy() {
        let base = std::env::temp_dir().join(format!("synccrate-dupdel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(base.join("Mods").join("CC")).unwrap();
        std::fs::write(base.join("Mods").join("a.package"), b"same").unwrap();
        std::fs::write(base.join("Mods").join("CC").join("b.package"), b"same").unwrap();
        let b = base.to_string_lossy().to_string();
        let cts = vec![mods_ct()];

        assert!(verify_kept_duplicate(&b, &cts, "Mods/CC/b.package", "Mods/a.package").is_ok());
        // Keeping the file itself would delete the only copy.
        assert!(verify_kept_duplicate(&b, &cts, "Mods/a.package", "Mods/a.package").is_err());
        // Kept copy changed since the scan.
        std::fs::write(base.join("Mods").join("a.package"), b"different").unwrap();
        assert!(verify_kept_duplicate(&b, &cts, "Mods/CC/b.package", "Mods/a.package").is_err());
        // Kept copy gone.
        std::fs::remove_file(base.join("Mods").join("a.package")).unwrap();
        assert!(verify_kept_duplicate(&b, &cts, "Mods/CC/b.package", "Mods/a.package").is_err());
        assert!(base.join("Mods").join("CC").join("b.package").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn game_folder_checks_refuse_roots_user_folders_and_other_games() {
        let tmp = utils::clean_path(std::fs::canonicalize(std::env::temp_dir()).unwrap());
        let docs = tmp.join("synccrate_docs");
        let sims = docs.join("Electronic Arts").join("The Sims 4");
        let protected = vec![(docs.clone(), "your Documents folder")];
        let others = vec![("Minecraft".to_string(), tmp.join(".minecraft"))];

        assert!(check_game_folder(&sims, &others, &protected).is_ok());
        assert!(check_game_folder(&docs, &others, &protected).unwrap_err().contains("Documents"));
        // Same as, inside, or containing another game's folder.
        assert!(check_game_folder(&tmp.join(".minecraft"), &others, &protected).is_err());
        assert!(check_game_folder(&tmp.join(".minecraft").join("mods"), &others, &protected).is_err());
        assert!(check_game_folder(&tmp, &others, &protected).is_err());
        #[cfg(target_os = "windows")]
        assert!(check_game_folder(std::path::Path::new(r"E:\"), &[], &[]).unwrap_err().contains("drive root"));
    }

    #[test]
    fn path_correction_climbs_out_of_known_subfolders() {
        let pc: crate::registry::PathCorrection = serde_json::from_value(serde_json::json!({
            "known_subfolders": ["Mods", "Saves"], "nested_corrections": {"AddOns": 2}
        }))
        .unwrap();
        let root = std::env::temp_dir().join("Game");
        assert_eq!(apply_path_correction(&pc, &root.join("mods")), root);
        assert_eq!(apply_path_correction(&pc, &root.join("Interface").join("AddOns")), root);
        assert_eq!(apply_path_correction(&pc, &root), root);
    }

    #[test]
    fn only_addon_variants_may_share_a_folder() {
        let reg = crate::registry::load_registry();
        let get = |id: &str| reg.games.iter().find(|g| g.id == id).unwrap();
        assert!(games_may_share(get("sims4-reshade"), get("sims4-gshade")));
        assert!(!games_may_share(get("sims4"), get("sims4-reshade")));
        assert!(!games_may_share(get("sims4"), get("sims3")));
    }

    #[test]
    fn saved_paths_are_not_replaced_by_state() {
        // save_game_config writes SAVED_PATHS, never AppState::game_paths, so an
        // auto-detected path can't overwrite a saved (possibly unplugged) one.
        let mut s = SavedPaths::default();
        s.paths.insert("sims4".into(), "X:/Custom/The Sims 4".into());
        s.user_set.insert("sims4".into());
        set_saved_paths(s);
        assert_eq!(user_set_path("sims4").as_deref(), Some("X:/Custom/The Sims 4"));
        assert_eq!(user_set_path("sims3"), None);
    }
}
