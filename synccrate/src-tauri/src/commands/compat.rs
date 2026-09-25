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
    let (def, base, manifest) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Ok(Vec::new());
        }
        let def = s.game_registry.games.iter().find(|g| g.id == game).cloned().ok_or("Unknown game")?;
        (def, s.active_game_path()?, s.local_manifest.clone())
    };
    tokio::task::spawn_blocking(move || {
        let exists = |rel: &str| crate::utils::safe_join(&base, rel).is_ok_and(|p| p.is_file());
        let mut out: Vec<CompatIssue> = compat::check_loader(&def, &manifest, exists).into_iter().collect();
        if def.id == "sims4" {
            let ini_path = Path::new(&base).join("Options.ini");
            let ini = std::fs::metadata(&ini_path)
                .ok()
                .filter(|m| m.is_file() && m.len() <= 1024 * 1024)
                .and_then(|_| std::fs::read_to_string(&ini_path).ok());
            let zip_has_package = |rel: &str| crate::utils::safe_join(&base, rel).is_ok_and(|p| compat::zip_contains_package(&p));
            out.extend(compat::check_sims4(&manifest, ini.as_deref(), zip_has_package));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Apply a fix offered by `check_compat` (only Options.ini for now). The
/// game must be closed: it rewrites Options.ini when it exits.
#[tauri::command]
pub async fn fix_compat_issue(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String, fix: String) -> Result<(), String> {
    fix_compat_issue_inner(state.inner(), &game, &fix).await
}

pub(crate) async fn fix_compat_issue_inner(state: &Arc<Mutex<AppState>>, game: &str, fix: &str) -> Result<(), String> {
    if fix != compat::FIX_SIMS4_ENABLE_MODS || game != "sims4" {
        return Err("Unknown fix.".into());
    }
    let (base, procs) = {
        let s = state.lock().await;
        let base = s.game_paths.get(game).cloned().ok_or("The Sims 4 folder isn't set.")?;
        let procs = s.game_registry.games.iter().find(|g| g.id == game).map(|g| g.process_names.clone()).unwrap_or_default();
        (base, procs)
    };
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return Err("Close The Sims 4 first: it rewrites Options.ini when it exits.".into());
    }
    tokio::task::spawn_blocking(move || enable_mods_in(Path::new(&base)))
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
