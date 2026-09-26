//! "Find the broken mod": the Sims community's 50/50 method, done for you.
//! Each round turns half of the remaining suspects off; the player checks the
//! game and says whether the problem is still there, so a culprit among n
//! mods is found in about log2(n) rounds instead of hours of moving folders.
//!
//! A unit is a top-level folder in the mods folder (a creator's package and
//! script mod belong together) or a loose file. Only mods that were on when
//! it started are ever touched, and restoring turns exactly those back on.
//! The state is saved after every round, so closing SyncCrate to play the
//! game (or a crash) doesn't lose which mods it turned off.

use crate::commands::files::{record_toggles, toggle_context, toggle_files};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub name: String,
    /// Relative paths while on.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bisect {
    game: String,
    base_path: String,
    started_at: u64,
    units: Vec<Unit>,
    /// Indices into `units` that may still hold the culprit.
    suspects: Vec<usize>,
    round: u32,
    /// Files this helper turned off: path while on -> path now.
    disabled: BTreeMap<String, String>,
    culprit: Option<usize>,
    #[serde(default)]
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BisectView {
    pub round: u32,
    /// Rounds still to go (0 once found).
    pub rounds_left: u32,
    pub total: usize,
    pub suspects: usize,
    /// Units off right now.
    pub off: usize,
    pub culprit: Option<Unit>,
    pub started_at: u64,
    /// Files the last step couldn't move.
    pub errors: Vec<String>,
}

/// Not mods: config and readme files in the mods folder (Sims 4's
/// Resource.cfg turned off would stop every mod from loading).
const NOT_MODS: &[&str] = &["cfg", "txt", "ini", "log", "md", "json", "html", "pdf", "png", "jpg", "jpeg", "url"];

fn rounds_for(n: usize) -> u32 {
    if n <= 1 { 0 } else { usize::BITS - (n - 1).leading_zeros() }
}

/// This round's off half: the second half of the suspects.
fn off_half(suspects: &[usize]) -> &[usize] {
    &suspects[suspects.len() / 2..]
}

/// After an answer: the half that still holds the culprit. "Still broken"
/// means it was among the ones left on.
fn narrow(suspects: &[usize], still_broken: bool) -> Vec<usize> {
    let mid = suspects.len() / 2;
    if still_broken { suspects[..mid].to_vec() } else { suspects[mid..].to_vec() }
}

/// Units from the enabled mod files in `folder`.
fn units_from(paths: &[String], folder: &str) -> Vec<Unit> {
    let prefix = format!("{}/", folder.trim_end_matches('/'));
    let mut by_name: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for p in paths {
        let Some(inner) = p.get(..prefix.len()).filter(|h| h.eq_ignore_ascii_case(&prefix)).map(|_| &p[prefix.len()..]) else { continue };
        let name = inner.split_once('/').map_or(inner, |(dir, _)| dir);
        by_name.entry(name.to_lowercase()).or_insert_with(|| (name.to_string(), Vec::new())).1.push(p.clone());
    }
    by_name
        .into_values()
        .map(|(name, mut files)| {
            files.sort();
            Unit { name, files }
        })
        .collect()
}

impl Bisect {
    fn view(&self) -> BisectView {
        BisectView {
            round: self.round,
            rounds_left: if self.culprit.is_some() { 0 } else { rounds_for(self.suspects.len()) },
            total: self.units.len(),
            suspects: self.suspects.len(),
            off: self.units.iter().filter(|u| u.files.iter().any(|f| self.disabled.contains_key(f))).count(),
            culprit: self.culprit.map(|i| self.units[i].clone()),
            started_at: self.started_at,
            errors: self.errors.clone(),
        }
    }

    /// Turn files on or off so exactly `off_units` are off. Returns the moves
    /// (for tags and the manifest); failures go to `errors`.
    fn apply(&mut self, off_units: &[usize], folder: &str, rename: bool) -> Vec<(String, String)> {
        let off: HashSet<&String> = off_units.iter().flat_map(|&i| self.units[i].files.iter()).collect();
        let to_enable: Vec<(String, String)> = self.disabled.iter().filter(|(on, _)| !off.contains(on)).map(|(a, b)| (a.clone(), b.clone())).collect();
        let to_disable: Vec<String> = off.iter().filter(|f| !self.disabled.contains_key(**f)).map(|f| (*f).clone()).collect();
        let mut moved = Vec::new();
        self.errors.clear();

        let now: Vec<String> = to_enable.iter().map(|(_, n)| n.clone()).collect();
        for ((on_path, _), o) in to_enable.iter().zip(toggle_files(&self.base_path, folder, &now, true, rename)) {
            match (o.new_path, o.error) {
                (Some(np), _) => {
                    if np != o.path {
                        moved.push((o.path.clone(), np));
                    }
                    self.disabled.remove(on_path);
                }
                // Deleted meanwhile: nothing left to turn back on.
                (None, Some(e)) if e == "File not found" => {
                    self.disabled.remove(on_path);
                }
                (None, e) => self.errors.push(format!("{on_path}: {}", e.unwrap_or_default())),
            }
        }
        for o in toggle_files(&self.base_path, folder, &to_disable, false, rename) {
            match (o.new_path, o.error) {
                // Already off (the user turned it off meanwhile): theirs, not ours to restore.
                (Some(np), _) if np == o.path => {}
                (Some(np), _) => {
                    moved.push((o.path.clone(), np.clone()));
                    self.disabled.insert(o.path, np);
                }
                (None, e) => self.errors.push(format!("{}: {}", o.path, e.unwrap_or_default())),
            }
        }
        moved
    }
}

fn record_path(game: &str) -> std::path::PathBuf {
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    let dir = crate::utils::config_root().join("synccrate").join("bisect");
    std::fs::create_dir_all(&dir).ok();
    dir.join(format!("{}.json", if safe.is_empty() { "unknown" } else { &safe }))
}

fn load(game: &str) -> Result<Option<Bisect>, String> {
    crate::utils::read_json_strict(&record_path(game))
}

fn save(b: &Bisect) -> Result<(), String> {
    crate::utils::write_json_atomic(&record_path(&b.game), b)
}

/// Renaming a mod the running game has open fails (and the answer needs the
/// game restarted anyway).
async fn require_game_closed(state: &Arc<Mutex<AppState>>, game: &str) -> Result<(), String> {
    let (procs, label) = {
        let st = state.lock().await;
        let procs = st.game_registry.games.iter().find(|g| g.id == game).map(|g| g.process_names.clone()).unwrap_or_default();
        (procs, st.game_label(game))
    };
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return Err(format!("Close {label} first: its mod files are in use."));
    }
    Ok(())
}

/// Run `apply` off the async workers, save, and update tags and the manifest.
async fn step(state: &Arc<Mutex<AppState>>, mut b: Bisect, off: Vec<usize>, folder: String, rename: bool, done: bool) -> Result<BisectView, String> {
    let (b, moved) = tokio::task::spawn_blocking(move || {
        let moved = b.apply(&off, &folder, rename);
        (b, moved)
    })
    .await
    .map_err(|e| e.to_string())?;
    record_toggles(state, &b.game, &moved).await;
    let finished = done && b.errors.is_empty();
    if finished {
        let _ = std::fs::remove_file(record_path(&b.game));
    } else {
        save(&b)?;
    }
    Ok(b.view())
}

#[tauri::command]
pub async fn bisect_status(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String) -> Result<Option<BisectView>, String> {
    let game = {
        let st = state.lock().await;
        crate::commands::files::resolve_game(&st, &game_id)?
    };
    Ok(load(&game)?.map(|b| b.view()))
}

#[tauri::command]
pub async fn bisect_start(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String) -> Result<BisectView, String> {
    crate::commands::backup::refuse_during_restore()?;
    let (game, base, folder, rename) = toggle_context(state.inner(), &game_id).await?;
    if load(&game)?.is_some() {
        return Err("A search is already running for this game.".into());
    }
    require_game_closed(state.inner(), &game).await?;
    let paths: Vec<String> = {
        let st = state.lock().await;
        st.local_manifest
            .files
            .keys()
            .filter(|p| !crate::sync::diff::is_disabled_path(p))
            .filter(|p| {
                let ext = crate::commands::files::effective_extension(std::path::Path::new(p.as_str()));
                !NOT_MODS.contains(&ext.as_str())
            })
            .cloned()
            .collect()
    };
    let units = units_from(&paths, &folder);
    if units.len() < 2 {
        return Err("There need to be at least two mods (or mod folders) that are on.".into());
    }
    let b = Bisect {
        game,
        base_path: base,
        started_at: crate::utils::timestamp_now(),
        suspects: (0..units.len()).collect(),
        units,
        round: 1,
        disabled: BTreeMap::new(),
        culprit: None,
        errors: Vec::new(),
    };
    let off = off_half(&b.suspects).to_vec();
    step(state.inner(), b, off, folder, rename, false).await
}

#[tauri::command]
pub async fn bisect_answer(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String, still_broken: bool) -> Result<BisectView, String> {
    crate::commands::backup::refuse_during_restore()?;
    let (game, base, folder, rename) = toggle_context(state.inner(), &game_id).await?;
    let mut b = load(&game)?.ok_or("No search is running for this game.")?;
    if b.base_path != base {
        return Err("The game folder changed since the search started. Stop it to turn the mods back on.".into());
    }
    if b.culprit.is_some() {
        return Err("The search is already done.".into());
    }
    require_game_closed(state.inner(), &game).await?;
    b.suspects = narrow(&b.suspects, still_broken);
    b.round += 1;
    let off = match b.suspects.as_slice() {
        // Found: everything else back on, the culprit stays off until the user decides.
        [one] => {
            b.culprit = Some(*one);
            vec![*one]
        }
        // "Still broken" with nothing left on can't happen with halves, but
        // never leave an empty search running.
        [] => return Err("Nothing left to check.".into()),
        s => off_half(s).to_vec(),
    };
    step(state.inner(), b, off, folder, rename, false).await
}

/// End the search: every mod it turned off goes back on, except the culprit
/// when `keep_culprit_off`.
#[tauri::command]
pub async fn bisect_stop(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String, keep_culprit_off: bool) -> Result<BisectView, String> {
    crate::commands::backup::refuse_during_restore()?;
    let (game, base, folder, rename) = toggle_context(state.inner(), &game_id).await?;
    let mut b = load(&game)?.ok_or("No search is running for this game.")?;
    if b.base_path != base {
        return Err("The game folder changed since the search started. Set it back to restore the mods.".into());
    }
    require_game_closed(state.inner(), &game).await?;
    // Kept off: the user's choice from now on, not the search's to restore.
    if let (true, Some(c)) = (keep_culprit_off, b.culprit) {
        let files: HashSet<String> = b.units[c].files.iter().cloned().collect();
        b.disabled.retain(|on, _| !files.contains(on));
    }
    step(state.inner(), b, Vec::new(), folder, rename, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn units_are_top_folders_or_loose_files() {
        let u = units_from(&strs(&["Mods/WW/a.package", "Mods/WW/b.ts4script", "Mods/hair.package", "Mods/ww/c.package", "Saves/x.save"]), "Mods");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].name, "hair.package");
        assert_eq!(u[1].files.len(), 3, "one creator folder, whatever its case");
    }

    #[test]
    fn halving_finds_any_culprit_in_log2_rounds() {
        for n in 2..40usize {
            for culprit in 0..n {
                let mut suspects: Vec<usize> = (0..n).collect();
                let mut rounds = 0;
                while suspects.len() > 1 {
                    let broken = !off_half(&suspects).contains(&culprit);
                    suspects = narrow(&suspects, broken);
                    rounds += 1;
                }
                assert_eq!(suspects, vec![culprit]);
                assert!(rounds <= rounds_for(n), "{n} mods took {rounds} rounds");
            }
        }
    }

    fn temp_game(name: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = crate::testutil::temp_dir(name);
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        dir
    }

    #[test]
    fn a_search_turns_off_halves_and_restores_exactly_what_it_turned_off() {
        let files = ["Mods/a.package", "Mods/b.package", "Mods/C/c.package", "Mods/C/c.ts4script", "Mods/d.package"];
        let dir = temp_game("bisect", &files);
        std::fs::write(dir.join("Mods/mine.package.disabled"), b"x").unwrap();
        let units = units_from(&strs(&files), "Mods");
        let mut b = Bisect {
            game: "sims4".into(),
            base_path: dir.to_string_lossy().into(),
            started_at: 0,
            suspects: (0..units.len()).collect(),
            units,
            round: 1,
            disabled: BTreeMap::new(),
            culprit: None,
            errors: Vec::new(),
        };
        let off = off_half(&b.suspects).to_vec();
        let moved = b.apply(&off, "Mods", true);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(moved.len(), b.disabled.len());
        assert_eq!(b.view().off, 2);
        for on in b.disabled.keys() {
            assert!(!dir.join(on).exists() && dir.join(format!("{on}.disabled")).exists());
        }

        // Fixed with those off: the culprit is among them; the others come back on.
        b.suspects = narrow(&b.suspects, false);
        let off = off_half(&b.suspects).to_vec();
        b.apply(&off, "Mods", true);
        assert_eq!(b.view().off, 1);

        // Restore everything: only what the search turned off.
        b.apply(&[], "Mods", true);
        assert!(b.disabled.is_empty());
        for f in files {
            assert!(dir.join(f).exists(), "{f} back on");
        }
        assert!(dir.join("Mods/mine.package.disabled").exists(), "the user's own disabled mod is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
