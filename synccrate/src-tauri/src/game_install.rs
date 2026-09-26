//! Whether a game is actually *installed*, as opposed to its mod/data folder
//! merely existing.
//!
//! Path detection (`utils::detect_game_path_from_def`) only checks that a
//! folder exists, and folders like `Documents/Electronic Arts/The Sims 4`,
//! `Documents/My Games/...` or a leftover `BepInEx` folder survive an
//! uninstall. A user reported many games showing as "Detected" that weren't
//! installed, so the UI's "Detected" now needs real install evidence from
//! Steam, the Windows uninstall list, Epic, GOG or a per-game marker.
//!
//! The expensive lookups are gathered once into an [`InstallContext`] and then
//! every registry game is checked against it.

use crate::registry::{DetectionStrategy, GameDefinition};
use crate::utils;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Why a game counts as installed (logged, and asserted in tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvidence {
    Steam(u32),
    Uninstall(String),
    Epic(String),
    Gog(String),
    Marker(String),
    RegistryDir(String),
}

/// Machine-wide install data, built once per detection pass.
#[derive(Debug, Default)]
pub struct InstallContext {
    /// Steam app ids with an `appmanifest_<id>.acf` whose install dir exists.
    pub steam_apps: HashSet<u32>,
    /// True when at least one Steam library exists (the Steam check "applies").
    pub steam_present: bool,
    /// Normalized `DisplayName`s from the Windows uninstall keys.
    pub uninstall_names: HashSet<String>,
    /// Normalized Epic `DisplayName`s whose `InstallLocation` exists.
    pub epic_names: HashSet<String>,
    /// Normalized GOG `gameName`s whose `path` exists.
    pub gog_names: HashSet<String>,
}

impl InstallContext {
    pub fn build() -> Self {
        let steamapps = utils::steam_steamapps_dirs();
        let norm = |v: Vec<String>| v.iter().map(|n| normalize_name(n)).filter(|n| !n.is_empty()).collect();
        Self {
            steam_apps: steam_installed_app_ids(&steamapps),
            steam_present: !steamapps.is_empty(),
            uninstall_names: norm(uninstall_display_names()),
            epic_names: norm(epic_manifest_dirs().iter().flat_map(|d| epic_installed_names(d)).collect()),
            gog_names: norm(gog_installed_names()),
        }
    }
}

/// Lowercase, drop ™®© and apostrophes, turn other punctuation into spaces,
/// `&` → "and", collapse whitespace and drop a leading "the".
/// "The Sims™ 4" and "the sims 4" both become "sims 4".
pub fn normalize_name(s: &str) -> String {
    let lower = s.to_lowercase().replace('&', " and ");
    let mut out = String::with_capacity(lower.len());
    for c in lower.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if matches!(c, '\'' | '\u{2019}' | '\u{2122}' | '\u{00ae}' | '\u{00a9}') {
            // Dropped rather than spaced: "Baldur's" -> "baldurs", "Sims™" -> "sims".
        } else {
            out.push(' ');
        }
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let words = match words.split_first() {
        Some((&"the", rest)) if !rest.is_empty() => rest,
        _ => &words[..],
    };
    words.join(" ")
}

/// Exact normalized match against the game's label or one of its
/// `install_names`. Exact on purpose: "The Sims 4 Seasons" (a DLC uninstall
/// entry) must not count as "The Sims 4".
fn matching_name(game: &GameDefinition, names: &HashSet<String>) -> Option<String> {
    std::iter::once(&game.label)
        .chain(game.install_names.iter())
        .map(|n| normalize_name(n))
        .find(|n| !n.is_empty() && names.contains(n))
}

/// Read a quoted `"key" "value"` pair from a Steam `.acf`/`.vdf` file.
fn vdf_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let t: Vec<&str> = line.split('"').collect();
        (t.len() >= 5 && t[1].eq_ignore_ascii_case(key)).then(|| t[3].replace("\\\\", "\\"))
    })
}

/// App ids with an `appmanifest_<id>.acf` in any of the given `steamapps`
/// dirs whose `installdir` exists. A stale or unreadable manifest alone
/// isn't proof: one without a readable `installdir` counted as installed,
/// which showed games as "Detected" that weren't there.
pub fn steam_installed_app_ids(steamapps_dirs: &[PathBuf]) -> HashSet<u32> {
    let mut ids = HashSet::new();
    for dir in steamapps_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let Some(id) = name
                .strip_prefix("appmanifest_")
                .and_then(|r| r.strip_suffix(".acf"))
                .and_then(|r| r.parse::<u32>().ok())
            else {
                continue;
            };
            let installdir = std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|c| vdf_value(&c, "installdir"));
            if installdir.is_some_and(|d| !d.is_empty() && dir.join("common").join(&d).is_dir()) {
                ids.insert(id);
            }
        }
    }
    ids
}

#[allow(unused_mut)]
fn epic_manifest_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
        out.push(PathBuf::from(base).join(r"Epic\EpicGamesLauncher\Data\Manifests"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        out.push(home.join("Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests"));
    }
    out
}

/// `DisplayName`s of Epic installs (`*.item` JSON manifests) whose
/// `InstallLocation` still exists and that aren't mid-install.
pub fn epic_installed_names(manifest_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(manifest_dir) else { return names };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("item")) {
            continue;
        }
        let Some(json) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        else {
            continue;
        };
        let name = json.get("DisplayName").and_then(|v| v.as_str()).unwrap_or("");
        let location = json.get("InstallLocation").and_then(|v| v.as_str()).unwrap_or("");
        let incomplete = json.get("bIsIncompleteInstall").and_then(|v| v.as_bool()).unwrap_or(false);
        if !name.is_empty() && !location.is_empty() && !incomplete && Path::new(location).exists() {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(target_os = "windows")]
const KEY_WOW64_64KEY: u32 = 0x0100;
#[cfg(target_os = "windows")]
const KEY_WOW64_32KEY: u32 = 0x0200;

/// `DisplayName` of every subkey of the three Uninstall roots (HKLM 64/32-bit
/// views and HKCU). Read natively: `reg query` goes through the OEM code page
/// and mangles "The Sims™ 4".
#[cfg(target_os = "windows")]
fn uninstall_display_names() -> Vec<String> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    const PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut names = Vec::new();
    for (root, view) in [
        (LOCAL_MACHINE, KEY_WOW64_64KEY),
        (LOCAL_MACHINE, KEY_WOW64_32KEY),
        (CURRENT_USER, 0),
    ] {
        let Ok(key) = root.options().read().access(view).open(PATH) else { continue };
        let Ok(subkeys) = key.keys() else { continue };
        for sub in subkeys {
            if let Ok(name) = key.options().read().open(&sub).and_then(|k| k.get_string("DisplayName")) {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(not(target_os = "windows"))]
fn uninstall_display_names() -> Vec<String> {
    Vec::new()
}

/// GOG Galaxy / offline installers register `gameName` + `path` under
/// `HKLM\SOFTWARE\(WOW6432Node\)GOG.com\Games\<id>`.
#[cfg(target_os = "windows")]
fn gog_installed_names() -> Vec<String> {
    use windows_registry::LOCAL_MACHINE;
    let mut names = Vec::new();
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        let Ok(key) = LOCAL_MACHINE.options().read().access(view).open(r"SOFTWARE\GOG.com\Games") else {
            continue;
        };
        let Ok(subkeys) = key.keys() else { continue };
        for sub in subkeys {
            let Ok(game) = key.options().read().open(&sub) else { continue };
            let (Ok(name), Ok(path)) = (game.get_string("gameName"), game.get_string("path")) else { continue };
            if Path::new(&path).exists() {
                names.push(name);
            }
        }
    }
    names
}

#[cfg(not(target_os = "windows"))]
fn gog_installed_names() -> Vec<String> {
    Vec::new()
}

/// Check one `install_markers` entry. `folders` are the game's detected and/or
/// user-set folders, used for relative markers.
fn marker_present(marker: &str, folders: &[&Path]) -> bool {
    if let Some((key, value)) = marker.split_once("::") {
        return utils::read_registry_string(key, value).is_some_and(|d| Path::new(&d).exists());
    }
    let expanded = utils::expand_path_vars(marker);
    if expanded.contains('%') || expanded.starts_with('~') {
        return false; // unknown variable on this machine
    }
    let path = Path::new(&expanded);
    if path.is_absolute() {
        path.exists()
    } else {
        folders.iter().any(|f| f.join(path).exists())
    }
}

/// The first piece of evidence that the game is installed, if any.
pub fn install_evidence(game: &GameDefinition, ctx: &InstallContext, folders: &[&Path]) -> Option<InstallEvidence> {
    if let Some(id) = game.steam_app_id.filter(|id| ctx.steam_apps.contains(id)) {
        return Some(InstallEvidence::Steam(id));
    }
    if let Some(n) = matching_name(game, &ctx.uninstall_names) {
        return Some(InstallEvidence::Uninstall(n));
    }
    if let Some(n) = matching_name(game, &ctx.epic_names) {
        return Some(InstallEvidence::Epic(n));
    }
    if let Some(n) = matching_name(game, &ctx.gog_names) {
        return Some(InstallEvidence::Gog(n));
    }
    if let Some(m) = game.install_markers.iter().find(|m| marker_present(m, folders)) {
        return Some(InstallEvidence::Marker(m.clone()));
    }
    // A `windows_registry` detection key (e.g. Maxis "Install Dir") is written
    // by the installer and removed on uninstall, so its base dir is evidence
    // even when the add-on subfolder the strategy targets is missing.
    for strategy in game.detection.iter().flat_map(|d| d.strategies.iter()) {
        if let DetectionStrategy::WindowsRegistry { keys, value, .. } = strategy {
            for key in keys {
                if let Some(dir) = utils::read_registry_string(key, value).filter(|d| Path::new(d).exists()) {
                    return Some(InstallEvidence::RegistryDir(dir));
                }
            }
        }
    }
    None
}

/// Final "installed on this PC" verdict used for the UI's Detected state.
/// `detected` is the auto-detected folder, `configured` one the user picked
/// in Settings (never an auto-detected path, which can be a leftover).
pub fn detect_installed(
    game: &GameDefinition,
    ctx: &InstallContext,
    detected: Option<&str>,
    configured: Option<&str>,
) -> bool {
    let folders: Vec<&Path> = [detected, configured].into_iter().flatten().map(Path::new).collect();
    // ReShade/GShade variants: the base game being installed isn't enough,
    // the add-on folder (checked via `require_any` during detection) must exist.
    let needs_addon = game.detection.as_ref().is_some_and(|d| !d.require_any.is_empty());
    if needs_addon && folders.is_empty() {
        return false;
    }
    if let Some(e) = install_evidence(game, ctx, &folders) {
        log::debug!("{} installed: {:?}", game.id, e);
        return true;
    }
    // Portable / cracked-free / custom-launcher installs leave no uninstall
    // entry. A folder the user explicitly chose is good enough: they know.
    if configured.is_some_and(|c| Path::new(c).is_dir()) {
        return true;
    }
    // Linux/macOS have no uninstall registry. Where no evidence source applies
    // (not a Steam game, or no Steam here), a found folder still counts so
    // those users don't lose detection entirely.
    let steam_applies = game.steam_app_id.is_some() && ctx.steam_present;
    !cfg!(target_os = "windows") && detected.is_some() && !steam_applies
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(json: &str) -> GameDefinition {
        serde_json::from_str(json).unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("synccrate_install_{}_{}", tag, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn normalize_strips_symbols_and_the() {
        assert_eq!(normalize_name("The Sims™ 4"), "sims 4");
        assert_eq!(normalize_name("the sims 4"), "sims 4");
        assert_eq!(normalize_name("Baldur's Gate 3"), "baldurs gate 3");
        assert_eq!(normalize_name("Call of Duty® 4: Modern Warfare®"), "call of duty 4 modern warfare");
        assert_eq!(normalize_name("Mount & Blade II: Bannerlord"), "mount and blade ii bannerlord");
        assert_eq!(normalize_name("  The  "), "the");
    }

    #[test]
    fn name_match_is_exact_so_dlc_does_not_count() {
        let g = game(r#"{"id":"sims4","label":"The Sims 4","family":"sims","content_types":[]}"#);
        let dlc: HashSet<String> = ["The Sims™ 4 Seasons", "The Sims™ 4 Get to Work"]
            .iter().map(|n| normalize_name(n)).collect();
        assert_eq!(matching_name(&g, &dlc), None);
        let base: HashSet<String> = [normalize_name("The Sims™ 4")].into_iter().collect();
        assert_eq!(matching_name(&g, &base), Some("sims 4".to_string()));
    }

    #[test]
    fn install_names_are_matched_too() {
        let g = game(r#"{"id":"wow_retail","label":"WoW Retail","family":"wow","content_types":[],
            "install_names":["World of Warcraft"]}"#);
        let ctx = InstallContext {
            uninstall_names: [normalize_name("World of Warcraft")].into_iter().collect(),
            ..Default::default()
        };
        assert_eq!(install_evidence(&g, &ctx, &[]), Some(InstallEvidence::Uninstall("world of warcraft".into())));
        // "World of Warcraft Classic" must not satisfy retail
        let ctx = InstallContext {
            uninstall_names: [normalize_name("World of Warcraft Classic")].into_iter().collect(),
            ..Default::default()
        };
        assert_eq!(install_evidence(&g, &ctx, &[]), None);
    }

    #[test]
    fn steam_acf_requires_manifest_and_installdir() {
        let lib = temp_dir("steam");
        let steamapps = lib.join("steamapps");
        std::fs::create_dir_all(steamapps.join("common").join("The Sims 4")).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_1222670.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"1222670\"\n\t\"installdir\"\t\t\"The Sims 4\"\n}\n",
        ).unwrap();
        // Stale manifest: its install dir is gone
        std::fs::write(
            steamapps.join("appmanifest_730.acf"),
            "\"AppState\"\n{\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n}\n",
        ).unwrap();
        // No installdir at all: not proof either.
        std::fs::write(steamapps.join("appmanifest_4000.acf"), "\"AppState\"
{
	\"appid\"		\"4000\"
}
").unwrap();
        std::fs::write(steamapps.join("libraryfolders.vdf"), "").unwrap();

        let ids = steam_installed_app_ids(&[steamapps.clone(), lib.join("missing")]);
        assert!(ids.contains(&1222670));
        assert!(!ids.contains(&730));
        assert_eq!(ids.len(), 1);

        let g = game(r#"{"id":"sims4","label":"The Sims 4","family":"sims","steam_app_id":1222670,"content_types":[]}"#);
        let ctx = InstallContext { steam_apps: ids, steam_present: true, ..Default::default() };
        assert_eq!(install_evidence(&g, &ctx, &[]), Some(InstallEvidence::Steam(1222670)));
        std::fs::remove_dir_all(&lib).ok();
    }

    #[test]
    fn epic_manifests_need_existing_location() {
        let dir = temp_dir("epic");
        let install = dir.join("GTAV");
        std::fs::create_dir_all(&install).unwrap();
        let item = |name: &str, loc: &Path, incomplete: bool| {
            serde_json::json!({"DisplayName": name, "InstallLocation": loc.to_string_lossy(), "bIsIncompleteInstall": incomplete}).to_string()
        };
        std::fs::write(dir.join("a.item"), item("Grand Theft Auto V", &install, false)).unwrap();
        std::fs::write(dir.join("b.item"), item("Fortnite", &dir.join("gone"), false)).unwrap();
        std::fs::write(dir.join("c.item"), item("Celeste", &install, true)).unwrap();
        std::fs::write(dir.join("d.item"), "not json").unwrap();
        std::fs::write(dir.join("e.txt"), item("Balatro", &install, false)).unwrap();
        assert_eq!(epic_installed_names(&dir), vec!["Grand Theft Auto V".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relative_markers_check_game_folders() {
        let dir = temp_dir("marker");
        std::fs::write(dir.join("Wow.exe"), b"").unwrap();
        let g = game(r#"{"id":"wow_custom","label":"WoW Custom Server","family":"wow","content_types":[],
            "install_markers":["Wow.exe"]}"#);
        let ctx = InstallContext::default();
        let folder = dir.to_string_lossy().to_string();
        assert!(detect_installed(&g, &ctx, None, Some(&folder)));
        assert!(!detect_installed(&g, &ctx, None, None));
        std::fs::remove_file(dir.join("Wow.exe")).unwrap();
        assert_eq!(install_evidence(&g, &ctx, &[dir.as_path()]), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_folder_without_evidence_is_not_installed() {
        let dir = temp_dir("leftover");
        let g = game(r#"{"id":"sims4","label":"The Sims 4","family":"sims","steam_app_id":1222670,"content_types":[]}"#);
        let folder = dir.to_string_lossy().to_string();
        assert!(!detect_installed(&g, &InstallContext::default(), Some(&folder), None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn user_configured_folder_counts_as_installed() {
        let dir = temp_dir("configured");
        let g = game(r#"{"id":"sims4","label":"The Sims 4","family":"sims","steam_app_id":1222670,"content_types":[]}"#);
        let folder = dir.to_string_lossy().to_string();
        assert!(detect_installed(&g, &InstallContext::default(), None, Some(&folder)));
        // A configured path whose drive is gone doesn't.
        std::fs::remove_dir_all(&dir).ok();
        assert!(!detect_installed(&g, &InstallContext::default(), None, Some(&folder)));
    }

    /// Manual check on a dev machine:
    /// `~/.synccrate_test.sh print_installed_games -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_installed_games() {
        let registry = crate::registry::load_registry();
        let ctx = InstallContext::build();
        println!("steam apps: {}, uninstall: {}, epic: {}, gog: {}",
            ctx.steam_apps.len(), ctx.uninstall_names.len(), ctx.epic_names.len(), ctx.gog_names.len());
        for g in &registry.games {
            let detected = if g.auto_detect { utils::detect_game_path_from_def(g) } else { None };
            let folders: Vec<&Path> = detected.iter().map(|d| Path::new(d.as_str())).collect();
            let evidence = install_evidence(g, &ctx, &folders);
            let installed = detect_installed(g, &ctx, detected.as_deref(), None);
            if installed || detected.is_some() {
                println!("{:<28} installed={:<5} folder={:<5} {:?}", g.id, installed, detected.is_some(), evidence);
            }
        }
    }
}
