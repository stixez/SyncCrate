//! Compatibility check commands (`crate::compat` has the checks).
use crate::compat::{self, CompatIssue};
use crate::state::AppState;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Problems that stop the active game's mods from loading. Other games
/// return nothing: only the active game has a current manifest.
#[tauri::command]
pub async fn check_compat(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<Vec<CompatIssue>, String> {
    check_compat_inner(state.inner(), &game).await
}

pub(crate) async fn check_compat_inner(state: &Arc<Mutex<AppState>>, game: &str) -> Result<Vec<CompatIssue>, String> {
    let (def, base, manifest, is_client) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Ok(Vec::new());
        }
        let def = s.game_registry.games.iter().find(|g| g.id == game).cloned().ok_or("Unknown game")?;
        (def, s.active_game_path()?, s.local_manifest.clone(), s.session_type == crate::state::SessionType::Client)
    };
    tokio::task::spawn_blocking(move || {
        let exists = |rel: &str| crate::utils::safe_join(&base, rel).is_ok_and(|p| p.is_file());
        let mut out: Vec<CompatIssue> = compat::check_loader(&def, &manifest, exists).into_iter().collect();
        if def.id == "sims4" {
            let env = sims4_env(&def, &base, &manifest, is_client);
            let zip_has_package = |rel: &str| crate::utils::safe_join(&base, rel).is_ok_and(|p| compat::zip_contains_package(&p));
            out.extend(compat::check_sims4(&manifest, &env, zip_has_package));
        }
        if compat::PARADOX_DESCRIPTOR_GAMES.contains(&def.id.as_str()) {
            out.extend(compat::check_paradox(&manifest, |rel| read_descriptor(&base, rel), |p, folder| descriptor_target(&base, p, folder)));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Read what the Sims 4 checks need besides the manifest.
fn sims4_env(def: &crate::registry::GameDefinition, base: &str, manifest: &crate::state::FileManifest, is_client: bool) -> compat::Sims4Env {
    let root = Path::new(base);
    let ini_path = root.join("Options.ini");
    let options_ini = std::fs::metadata(&ini_path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= 1024 * 1024)
        .and_then(|_| std::fs::read_to_string(&ini_path).ok());
    let (resource_cfg, resource_cfg_missing) = match find_resource_cfg(root) {
        Some(p) => (
            std::fs::symlink_metadata(&p)
                .ok()
                .filter(|m| m.is_file() && m.len() <= MAX_RESOURCE_CFG_BYTES)
                .and_then(|_| std::fs::read(&p).ok())
                .map(|b| String::from_utf8_lossy(&b).into_owned()),
            false,
        ),
        None => (None, root.join("Mods").is_dir()),
    };
    compat::Sims4Env {
        options_ini,
        resource_cfg,
        resource_cfg_missing,
        scripts: script_modules(base, manifest),
        tray_in_mods: tray_candidates(root),
        reports: error_reports(root).into_iter().map(|(_, r)| r).collect(),
        patch_time: crate::commands::files::patch_time_of(def, base),
        is_client,
    }
}

const MAX_RESOURCE_CFG_BYTES: u64 = 64 * 1024;
const MAX_SCRIPTS_INSPECTED: usize = 2000;
/// Entries walked in Mods looking for Tray files (Mods of 100k+ files exist).
const MAX_WALK_ENTRIES: usize = 400_000;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REPORTS: usize = 10;

/// `Mods/Resource.cfg` in whatever case it was written.
fn find_resource_cfg(root: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(root.join("Mods"))
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("resource.cfg"))
        .map(|e| e.path())
}

/// Modules per script mod, cached by path + size + date: the check runs
/// after every scan, and opening every script archive each time added up.
fn script_modules(base: &str, manifest: &crate::state::FileManifest) -> Vec<compat::ScriptModules> {
    use std::collections::{BTreeSet, HashMap};
    static CACHE: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (u64, u64, BTreeSet<String>)>>> = std::sync::LazyLock::new(Default::default);
    let scripts = manifest.files.values().filter(|f| {
        let p = f.relative_path.as_str();
        p.starts_with("Mods/") && !crate::sync::diff::is_disabled_path(p) && crate::commands::files::effective_extension(Path::new(p)) == "ts4script"
    });
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    for f in scripts.take(MAX_SCRIPTS_INSPECTED) {
        let Ok(path) = crate::utils::safe_join(base, &f.relative_path) else { continue };
        let key = path.to_string_lossy().into_owned();
        let modules = match cache.get(&key) {
            Some((size, modified, m)) if *size == f.size && *modified == f.modified => m.clone(),
            _ => {
                let m = crate::mod_meta::open_bounded_zip(&path, 256 * 1024 * 1024)
                    .map(|z| compat::script_module_names(z.file_names()))
                    .unwrap_or_default();
                cache.insert(key, (f.size, f.modified, m.clone()));
                m
            }
        };
        out.push(compat::ScriptModules { path: f.relative_path.clone(), modified: f.modified, modules });
    }
    if cache.len() > 4 * MAX_SCRIPTS_INSPECTED {
        cache.clear();
    }
    out
}

/// Files under Mods with a Tray (or `.bpi`) extension, as `Mods/...` paths.
/// The Mods scan doesn't list them, so this walks the folder itself.
fn tray_candidates(root: &Path) -> Vec<String> {
    let mods = root.join("Mods");
    walkdir::WalkDir::new(&mods)
        .follow_links(false)
        .max_depth(16)
        .into_iter()
        .take(MAX_WALK_ENTRIES)
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
            let ext = crate::commands::files::effective_extension(Path::new(&rel));
            (ext == "bpi" || compat::SIMS4_TRAY_EXTENSIONS.contains(&ext.as_str())).then_some(rel)
        })
        .collect()
}

/// The game's error reports (`lastException.txt`, `lastUIException_*.txt`,
/// ...) in its folder, newest first.
fn error_reports(root: &Path) -> Vec<(std::path::PathBuf, compat::ErrorReport)> {
    let Ok(dir) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<(std::path::PathBuf, u64)> = dir
        .flatten()
        .filter(|e| is_error_report(&e.file_name().to_string_lossy()))
        .filter_map(|e| {
            let meta = std::fs::symlink_metadata(e.path()).ok().filter(|m| m.is_file())?;
            let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some((e.path(), modified))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1));
    found
        .into_iter()
        .take(MAX_REPORTS)
        .map(|(p, modified)| {
            let text = std::fs::symlink_metadata(&p)
                .ok()
                .filter(|m| m.len() <= MAX_REPORT_BYTES)
                .and_then(|_| std::fs::read(&p).ok())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            (p, compat::ErrorReport { modified, text })
        })
        .collect()
}

fn is_error_report(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("last") && n.contains("exception") && n.ends_with(".txt")
}

/// Whether an absolute descriptor path is this game's own `mod/<folder>`
/// (same folder once resolved), missing here, or some other folder.
fn descriptor_target(base: &str, path: &str, folder: &str) -> compat::DescriptorTarget {
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| Path::new(path).to_path_buf()),
        None => Path::new(path).to_path_buf(),
    };
    let Ok(target) = std::fs::canonicalize(&expanded) else { return compat::DescriptorTarget::Missing };
    let ours = std::fs::canonicalize(Path::new(base).join("mod").join(folder));
    if ours.is_ok_and(|o| o == target) { compat::DescriptorTarget::ThisModFolder } else { compat::DescriptorTarget::Elsewhere }
}

fn read_descriptor(base: &str, rel: &str) -> Option<String> {
    let p = crate::utils::safe_join(base, rel).ok()?;
    let meta = std::fs::symlink_metadata(&p).ok()?;
    (meta.is_file() && meta.len() <= compat::MAX_DESCRIPTOR_BYTES).then(|| std::fs::read_to_string(&p).ok()).flatten()
}

/// Apply a fix offered by `check_compat`: Sims 4 files (the game must be
/// closed: it rewrites Options.ini when it exits and holds its mods open) or
/// Paradox mod paths. Returns what was done, for the toast.
#[tauri::command]
pub async fn fix_compat_issue(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String, fix: String) -> Result<Option<String>, String> {
    fix_compat_issue_inner(state.inner(), &game, &fix).await
}

const SIMS4_FIXES: &[&str] = &[
    compat::FIX_SIMS4_ENABLE_MODS,
    compat::FIX_SIMS4_RESOURCE_CFG,
    compat::FIX_SIMS4_DISABLE_OLDER_SCRIPTS,
    compat::FIX_SIMS4_MOVE_TRAY_FILES,
    compat::FIX_SIMS4_MOVE_CC_TO_MODS,
    compat::FIX_SIMS4_CLEAR_REPORTS,
];

pub(crate) async fn fix_compat_issue_inner(state: &Arc<Mutex<AppState>>, game: &str, fix: &str) -> Result<Option<String>, String> {
    if fix == compat::FIX_PARADOX_RELATIVE_PATHS && compat::PARADOX_DESCRIPTOR_GAMES.contains(&game) {
        return fix_paradox_paths(state, game).await.map(|()| None);
    }
    if game != "sims4" || !SIMS4_FIXES.contains(&fix) {
        return Err("Unknown fix.".into());
    }
    let moves_files = fix != compat::FIX_SIMS4_ENABLE_MODS && fix != compat::FIX_SIMS4_CLEAR_REPORTS;
    if moves_files {
        crate::commands::backup::refuse_during_restore()?;
    }
    let (base, procs, manifest, is_client) = {
        let s = state.lock().await;
        let base = s.game_paths.get(game).cloned().ok_or("The Sims 4 folder isn't set.")?;
        if moves_files {
            if s.is_any_syncing() {
                return Err("Wait for the sync to finish first.".into());
            }
            if s.active_game != game {
                return Err("Open The Sims 4 in SyncCrate first.".into());
            }
        }
        let procs = s.game_registry.games.iter().find(|g| g.id == game).map(|g| g.process_names.clone()).unwrap_or_default();
        (base, procs, s.local_manifest.clone(), s.session_type == crate::state::SessionType::Client)
    };
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return Err(if fix == compat::FIX_SIMS4_ENABLE_MODS {
            "Close The Sims 4 first: it rewrites Options.ini when it exits.".into()
        } else {
            "Close The Sims 4 first: it keeps its mod and Tray files open while it runs.".into()
        });
    }
    if fix == compat::FIX_SIMS4_DISABLE_OLDER_SCRIPTS {
        return disable_older_scripts(state, manifest).await.map(Some);
    }
    if fix == compat::FIX_SIMS4_MOVE_CC_TO_MODS && is_client {
        return Err("These came from your host: ask them to move the files into Mods, or the next sync brings them back.".into());
    }
    let fix = fix.to_string();
    tokio::task::spawn_blocking(move || {
        let root = Path::new(&base);
        match fix.as_str() {
            compat::FIX_SIMS4_ENABLE_MODS => enable_mods_in(root).map(|()| None),
            compat::FIX_SIMS4_RESOURCE_CFG => write_resource_cfg(root).map(Some),
            compat::FIX_SIMS4_MOVE_TRAY_FILES => {
                let files = compat::tray_files(&tray_candidates(root));
                move_flat(root, &files, "Tray").map(|(moved, kept)| Some(moved_message(moved, kept, "Tray")))
            }
            compat::FIX_SIMS4_MOVE_CC_TO_MODS => {
                let files = compat::cc_in_tray(&manifest);
                move_flat(root, &files, "Mods").map(|(moved, kept)| Some(moved_message(moved, kept, "Mods")))
            }
            _ => clear_error_reports(root).map(Some),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Turn off all but the newest copy of each duplicated script mod, the same
/// way the Content page does (reversible there).
async fn disable_older_scripts(state: &Arc<Mutex<AppState>>, manifest: crate::state::FileManifest) -> Result<String, String> {
    let (game_id, base, folder, rename) = crate::commands::files::toggle_context(state, "sims4").await?;
    let outcomes = tokio::task::spawn_blocking(move || {
        let older = compat::older_script_copies(&script_modules(&base, &manifest));
        crate::commands::files::toggle_files(&base, &folder, &older, false, rename)
    })
    .await
    .map_err(|e| e.to_string())?;
    let moved: Vec<(String, String)> = outcomes
        .iter()
        .filter_map(|o| o.new_path.as_ref().filter(|n| **n != o.path).map(|n| (o.path.clone(), n.clone())))
        .collect();
    crate::commands::files::record_toggles(state, &game_id, &moved).await;
    let failed: Vec<String> = outcomes.iter().filter_map(|o| o.error.as_ref().map(|e| format!("{}: {e}", o.path))).collect();
    if !failed.is_empty() {
        return Err(format!("Turned off {}, but couldn't turn off {}", copies(moved.len()), failed.join("; ")));
    }
    Ok(format!("Turned off {}.", copies(moved.len())))
}

fn copies(n: usize) -> String {
    format!("{n} older {}", if n == 1 { "copy" } else { "copies" })
}

/// Put the standard Resource.cfg in Mods. An existing one (empty or broken,
/// or the check wouldn't offer this) is kept as `.synccrate-backup`.
pub(crate) fn write_resource_cfg(root: &Path) -> Result<String, String> {
    let mods = root.join("Mods");
    std::fs::create_dir_all(&mods).map_err(|e| e.to_string())?;
    let target = match find_resource_cfg(root) {
        Some(existing) => {
            std::fs::copy(&existing, mods.join("Resource.cfg.synccrate-backup")).map_err(|e| format!("Couldn't back up Resource.cfg: {e}"))?;
            existing
        }
        None => mods.join("Resource.cfg"),
    };
    // A name scans skip (`is_synccrate_temp`), so it can't get synced.
    let tmp = mods.join(".Resource.cfg.synccrate-restore.tmp");
    std::fs::write(&tmp, compat::SIMS4_RESOURCE_CFG).map_err(|e| e.to_string())?;
    crate::utils::make_replaceable(&target);
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok("Resource.cfg is in place. Start the game again for it to take effect.".into())
}

/// Move `files` (relative to `root`) straight into `root/<dest>`, the only
/// place the game reads them from (Tray has no subfolders; Mods is fine at
/// the top). A file whose name is already taken there stays put: never
/// overwrite. Returns (moved, kept).
pub(crate) fn move_flat(root: &Path, files: &[String], dest: &str) -> Result<(usize, usize), String> {
    let dest_dir = root.join(dest);
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let (mut moved, mut kept) = (0, 0);
    let mut errors = Vec::new();
    for rel in files {
        let src = crate::utils::safe_join(&root.to_string_lossy(), rel)?;
        let Some(name) = src.file_name() else { continue };
        let target = dest_dir.join(name);
        if target.exists() {
            kept += 1;
            continue;
        }
        match std::fs::rename(&src, &target) {
            Ok(()) => moved += 1,
            Err(e) => errors.push(format!("{rel}: {e}")),
        }
    }
    if !errors.is_empty() {
        return Err(format!("Moved {moved}, but couldn't move {}", errors.join("; ")));
    }
    Ok((moved, kept))
}

fn moved_message(moved: usize, kept: usize, dest: &str) -> String {
    let mut s = format!("Moved {moved} file{} into {dest}.", if moved == 1 { "" } else { "s" });
    if kept > 0 {
        s.push_str(&format!(" {kept} stayed where they were: a file with the same name is already in {dest}."));
    }
    s
}

/// Every report, not just the newest ones the check reads.
fn clear_error_reports(root: &Path) -> Result<String, String> {
    let mut deleted = 0;
    for e in std::fs::read_dir(root).map_err(|e| e.to_string())?.flatten() {
        if !is_error_report(&e.file_name().to_string_lossy()) || !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        std::fs::remove_file(e.path()).map_err(|err| format!("Couldn't delete {}: {err}", e.path().display()))?;
        deleted += 1;
    }
    Ok(format!("Deleted {deleted} error report{}.", if deleted == 1 { "" } else { "s" }))
}

/// Make this PC's absolute descriptor paths portable (`path="mod/<folder>"`),
/// keeping the old versions in file history. Only descriptors pointing at a
/// folder on this PC are touched: rewriting ones that came from a host would
/// just differ from the host again on the next sync.
async fn fix_paradox_paths(state: &Arc<Mutex<AppState>>, game: &str) -> Result<(), String> {
    crate::commands::backup::refuse_during_restore()?;
    let (base, manifest) = {
        let s = state.lock().await;
        if s.is_any_syncing() {
            return Err("Wait for the sync to finish first.".into());
        }
        if s.active_game != game {
            return Err("Open this game in SyncCrate first.".into());
        }
        (s.active_game_path()?, s.local_manifest.clone())
    };
    let game = game.to_string();
    tokio::task::spawn_blocking(move || {
        let (own, _) = compat::absolute_descriptors(&manifest, |rel| read_descriptor(&base, rel), |p, folder| descriptor_target(&base, p, folder));
        if own.is_empty() {
            return Ok(());
        }
        use crate::commands::history;
        let root = crate::utils::backups_dir();
        let now = crate::utils::timestamp_now();
        let targets: Vec<String> = own.iter().map(|(d, _)| d.clone()).collect();
        let capture = crate::commands::sync::read_sync_config().keep_file_history.then(|| {
            let id = uuid::Uuid::new_v4().to_string();
            if let Err(e) = history::begin_capture(&root, &game, &base, &targets, "", &id, now) {
                log::warn!("File history: couldn't keep mod descriptors: {}", e);
            }
            id
        });
        let mut changed = Vec::new();
        let mut errors = Vec::new();
        for (rel, folder) in &own {
            let res = (|| -> Result<(), String> {
                let path = crate::utils::safe_join(&base, rel)?;
                let text = read_descriptor(&base, rel).ok_or("couldn't read it")?;
                // A name scans already skip (`is_synccrate_temp`), so a
                // crash between write and rename can't get it synced.
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let tmp = path.with_file_name(format!(".{name}.synccrate-restore.tmp"));
                std::fs::write(&tmp, compat::relative_descriptor(&text, folder)).map_err(|e| e.to_string())?;
                crate::utils::make_replaceable(&path);
                std::fs::rename(&tmp, &path).map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    e.to_string()
                })
            })();
            match res {
                Ok(()) => changed.push((rel.clone(), history::REASON_PATH_FIXED)),
                Err(e) => errors.push(format!("{rel}: {e}")),
            }
        }
        if let Some(id) = capture {
            if let Err(e) = history::finish_local(&root, &game, &id, &changed, crate::utils::timestamp_now()) {
                log::warn!("File history: {}", e);
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(format!("Couldn't fix {}", errors.join("; "))) }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Rewrite Options.ini with mods on, keeping a copy of the original next to it.
pub(crate) fn enable_mods_in(base: &Path) -> Result<(), String> {
    let ini = base.join("Options.ini");
    let meta = std::fs::symlink_metadata(&ini).map_err(|_| "Options.ini not found. Start the game once, then try again.".to_string())?;
    if !meta.file_type().is_file() || meta.len() > 1024 * 1024 {
        return Err("Options.ini doesn't look like the game's settings file.".into());
    }
    let text = std::fs::read_to_string(&ini).map_err(|e| e.to_string())?;
    std::fs::copy(&ini, base.join("Options.ini.synccrate-backup")).map_err(|e| format!("Couldn't back up Options.ini: {e}"))?;
    let tmp = base.join("Options.ini.synccrate-tmp");
    std::fs::write(&tmp, compat::sims4_enable_mods(&text)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &ini).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_then_fix_on_a_real_scanned_sims4_folder() {
        let _g = crate::testutil::e2e_guard().await;
        let dir = crate::testutil::temp_dir("compat-sims4");
        crate::testutil::write_file(&dir, "Mods/A/B/deep.ts4script", b"PK");
        crate::testutil::write_file(&dir, "Mods/ok.package", b"DBPF");
        std::fs::write(dir.join("Options.ini"), "[options]
modsdisabled = 1
scriptmodsenabled = 0
").unwrap();
        let state = crate::testutil::make_state("sims4", &dir);
        crate::commands::files::scan_files_inner(&state, None, true).await.unwrap();

        let issues = check_compat_inner(&state, "sims4").await.unwrap();
        let kinds: Vec<_> = issues.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, vec!["sims4_mods_disabled", "sims4_script_too_deep"]);
        assert!(check_compat_inner(&state, "valheim").await.unwrap().is_empty(), "only the active game");

        fix_compat_issue_inner(&state, "sims4", compat::FIX_SIMS4_ENABLE_MODS).await.unwrap();
        let kinds: Vec<_> = check_compat_inner(&state, "sims4").await.unwrap().into_iter().map(|i| i.kind).collect();
        assert_eq!(kinds, vec!["sims4_script_too_deep".to_string()], "options fixed; the file placement still needs the user");
        assert!(fix_compat_issue_inner(&state, "sims4", "rm -rf").await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn script_zip(module: &str) -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        z.start_file(format!("{module}/__init__.pyc"), zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"pyc").unwrap();
        z.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn sims4_health_fixes_on_a_real_folder() {
        use crate::testutil::{write_file, write_file_mtime};
        let _g = crate::testutil::e2e_guard().await;
        let dir = crate::testutil::temp_dir("compat-health");
        // Game updated long ago; both script copies predate the error report.
        write_file_mtime(&dir, "GameVersion.txt", b"1.110", 1_000_000);
        write_file(&dir, "Options.ini", b"[options]\nmodsdisabled = 0\nscriptmodsenabled = 1\n");
        write_file_mtime(&dir, "Mods/New/mccc.ts4script", &script_zip("mc_cmd_center"), 3_000_000);
        write_file_mtime(&dir, "Mods/Old/mccc_old.ts4script", &script_zip("mc_cmd_center"), 2_000_000);
        write_file(&dir, "Mods/CC/hair.package", b"DBPF");
        write_file(&dir, "Mods/Lots/0x1!0xab.blueprint", b"bp");
        write_file(&dir, "Mods/Lots/0x1!0xab.bpi", b"img");
        write_file(&dir, "Tray/sofa.package", b"DBPF");
        write_file(&dir, "lastException.txt", br"File 'C:\Mods\Old\mccc_old.ts4script\mc_cmd_center\x.py'");
        let state = crate::testutil::make_state("sims4", &dir);
        crate::commands::files::scan_files_inner(&state, None, true).await.unwrap();

        let issues = check_compat_inner(&state, "sims4").await.unwrap();
        let kinds: Vec<_> = issues.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, ["sims4_resource_cfg_missing", "sims4_duplicate_scripts", "sims4_cc_in_tray", "sims4_tray_in_mods", "sims4_script_errors"]);
        assert_eq!(issues[4].paths, ["Mods/Old/mccc_old.ts4script"]);

        for fix in [
            compat::FIX_SIMS4_CLEAR_REPORTS,
            compat::FIX_SIMS4_RESOURCE_CFG,
            compat::FIX_SIMS4_DISABLE_OLDER_SCRIPTS,
            compat::FIX_SIMS4_MOVE_CC_TO_MODS,
            compat::FIX_SIMS4_MOVE_TRAY_FILES,
        ] {
            let msg = fix_compat_issue_inner(&state, "sims4", fix).await.unwrap();
            assert!(msg.is_some(), "{fix} says what it did");
        }
        assert!(!dir.join("lastException.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.join("Mods/Resource.cfg")).unwrap(), compat::SIMS4_RESOURCE_CFG);
        assert!(dir.join("Mods/Old/mccc_old.ts4script.disabled").is_file() && dir.join("Mods/New/mccc.ts4script").is_file(), "the newest copy stays on");
        assert!(dir.join("Mods/sofa.package").is_file() && dir.join("Tray/0x1!0xab.blueprint").is_file() && dir.join("Tray/0x1!0xab.bpi").is_file());

        crate::commands::files::scan_files_inner(&state, None, true).await.unwrap();
        assert!(check_compat_inner(&state, "sims4").await.unwrap().is_empty());

        // Never overwrite: a name already in Tray stays in Mods.
        write_file(&dir, "Mods/Lots/0x1!0xab.blueprint", b"another");
        assert!(fix_compat_issue_inner(&state, "sims4", compat::FIX_SIMS4_MOVE_TRAY_FILES).await.unwrap().unwrap().contains("1 stayed"));
        assert_eq!(std::fs::read(dir.join("Tray/0x1!0xab.blueprint")).unwrap(), b"bp");
        // A client may not move the host's Tray CC.
        write_file(&dir, "Tray/rug.package", b"DBPF");
        crate::commands::files::scan_files_inner(&state, None, true).await.unwrap();
        state.lock().await.session_type = crate::state::SessionType::Client;
        assert!(fix_compat_issue_inner(&state, "sims4", compat::FIX_SIMS4_MOVE_CC_TO_MODS).await.unwrap_err().contains("host"));
        assert!(dir.join("Tray/rug.package").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_resource_cfg_is_replaced_and_kept() {
        let dir = crate::testutil::temp_dir("compat-cfg");
        crate::testutil::write_file(&dir, "Mods/resource.cfg", b"Priority 500\r\n");
        write_resource_cfg(&dir).unwrap();
        // Written over the existing name (its case kept), old one backed up.
        assert_eq!(std::fs::read_to_string(dir.join("Mods/resource.cfg")).unwrap(), compat::SIMS4_RESOURCE_CFG);
        assert_eq!(std::fs::read(dir.join("Mods/Resource.cfg.synccrate-backup")).unwrap(), b"Priority 500\r\n");
        assert!(!dir.join("Mods/.Resource.cfg.synccrate-restore.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn paradox_absolute_paths_are_made_portable() {
        let _g = crate::testutil::e2e_guard().await;
        let dir = crate::testutil::temp_dir("compat-paradox");
        let own = format!("{}/mod/Mine", dir.to_string_lossy().replace('\\', "/"));
        crate::testutil::write_file(&dir, "mod/Mine/descriptor.mod", b"name=\"Mine\"\n");
        crate::testutil::write_file(&dir, "mod/Mine.mod", format!("name=\"Mine\"\r\npath=\"{own}\"\r\n").as_bytes());
        crate::testutil::write_file(&dir, "mod/Theirs/descriptor.mod", b"name=\"Theirs\"\n");
        crate::testutil::write_file(&dir, "mod/Theirs.mod", b"path=\"C:/Users/someone-else/Documents/Paradox Interactive/Hearts of Iron IV/mod/Theirs\"\n");
        let state = crate::testutil::make_state("hearts_of_iron_4", &dir);
        crate::commands::files::scan_files_inner(&state, None, true).await.unwrap();

        let kinds: Vec<String> = check_compat_inner(&state, "hearts_of_iron_4").await.unwrap().into_iter().map(|i| i.kind).collect();
        assert_eq!(kinds, ["paradox_absolute_paths", "paradox_foreign_paths"]);

        fix_compat_issue_inner(&state, "hearts_of_iron_4", compat::FIX_PARADOX_RELATIVE_PATHS).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("mod/Mine.mod")).unwrap(), "name=\"Mine\"\r\npath=\"mod/Mine\"\r\n");
        assert!(std::fs::read_to_string(dir.join("mod/Theirs.mod")).unwrap().contains("someone-else"), "a host's descriptor is left for the host to fix");
        assert!(!dir.join("mod/.Mine.mod.synccrate-restore.tmp").exists());
        assert!(fix_compat_issue_inner(&state, "sims4", compat::FIX_PARADOX_RELATIVE_PATHS).await.is_err(), "Paradox games only");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enable_mods_rewrites_options_ini_and_keeps_a_backup() {
        let dir = crate::testutil::temp_dir("compat-fix");
        assert!(enable_mods_in(&dir).unwrap_err().contains("not found"));
        let orig = "[options]\r\nmodsdisabled = 1\r\nscriptmodsenabled = 0\r\n";
        std::fs::write(dir.join("Options.ini"), orig).unwrap();
        enable_mods_in(&dir).unwrap();
        let now = std::fs::read_to_string(dir.join("Options.ini")).unwrap();
        assert_eq!(compat::sims4_mod_flags(&now), (Some(false), Some(true)));
        assert_eq!(std::fs::read_to_string(dir.join("Options.ini.synccrate-backup")).unwrap(), orig);
        assert!(!dir.join("Options.ini.synccrate-tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
