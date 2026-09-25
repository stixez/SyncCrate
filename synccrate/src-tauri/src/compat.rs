//! Compatibility checks: problems that make synced files not *work*, even
//! though both PCs have the same ones ("we have the same mods but it still
//! doesn't load"). All checks read the scanned manifest plus a couple of
//! known files; the only write is the explicit Sims 4 Options.ini fix.
//!
//! - Missing mod loader (registry `mod_loader`): e.g. BepInEx plugins but
//!   no BepInEx core, SMAPI mods but no SMAPI.
//! - The Sims 4: mods / script mods switched off in Options.ini (the game
//!   does this itself after every patch), `.ts4script` more than one folder
//!   deep, `.package` more than five folders deep (both silently ignored by
//!   the game), and `.zip` archives holding `.package` files (never loaded;
//!   they need extracting).
use crate::registry::GameDefinition;
use crate::state::FileManifest;
use serde::Serialize;
use std::path::Path;

/// Paths listed per issue; the count says how many there really are.
const MAX_PATHS: usize = 50;
const MAX_ZIPS_INSPECTED: usize = 500;
pub const FIX_SIMS4_ENABLE_MODS: &str = "sims4_enable_mods";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompatIssue {
    pub kind: String,
    /// "error" (won't work) or "warn" (probably won't).
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub count: usize,
    pub paths: Vec<String>,
    /// A fix the app can apply (`fix_compat_issue`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

fn is_disabled(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(crate::commands::files::DISABLED_SUFFIX)
}

fn ext(path: &str) -> String {
    crate::commands::files::effective_extension(Path::new(path))
}

fn issue(kind: &str, severity: &str, title: String, detail: &str, mut paths: Vec<String>) -> CompatIssue {
    paths.sort();
    let count = paths.len();
    paths.truncate(MAX_PATHS);
    CompatIssue { kind: kind.into(), severity: severity.into(), title, detail: detail.into(), count, paths, fix: None, url: None }
}

/// Files of `content_type` in the manifest, by the registry's folder.
fn files_in<'a>(game: &'a GameDefinition, manifest: &'a FileManifest, content_type: &'a str) -> impl Iterator<Item = &'a String> + 'a {
    manifest.files.keys().filter(move |path| {
        crate::sync::diff::content_type_for(&game.content_types, path).is_some_and(|(ct, _)| ct.id == content_type)
    })
}

/// Registry `mod_loader`: mods present, loader files absent.
pub fn check_loader(game: &GameDefinition, manifest: &FileManifest, exists: impl Fn(&str) -> bool) -> Option<CompatIssue> {
    let loader = game.mod_loader.as_ref()?;
    let mods: Vec<String> = files_in(game, manifest, &loader.content_type).filter(|p| !is_disabled(p)).cloned().collect();
    if mods.is_empty() || loader.any_of.iter().any(|p| exists(p)) {
        return None;
    }
    let mut i = issue(
        "missing_loader",
        "error",
        format!("{} isn't installed, so these {} mod files won't load", loader.name, mods.len()),
        &format!("{} needs {} to load mods. Install it into the game folder (a friend's copy syncs only the mods, not the loader).", game.label, loader.name),
        mods,
    );
    i.url = loader.url.clone();
    Some(i)
}

/// Folders between `Mods/` and the file (`Mods/a/b/x.package` → 2).
fn depth_under(root: &str, path: &str) -> Option<usize> {
    let rest = path.strip_prefix(root)?.strip_prefix('/')?;
    Some(rest.matches('/').count())
}

/// Parse Options.ini's `modsdisabled` / `scriptmodsenabled` (None if absent).
pub fn sims4_mod_flags(options_ini: &str) -> (Option<bool>, Option<bool>) {
    let (mut mods_disabled, mut scripts_enabled) = (None, None);
    for line in options_ini.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let on = v.trim() == "1";
        match k.trim().to_ascii_lowercase().as_str() {
            "modsdisabled" => mods_disabled = Some(on),
            "scriptmodsenabled" => scripts_enabled = Some(on),
            _ => {}
        }
    }
    (mods_disabled, scripts_enabled)
}

/// Options.ini with mods and script mods switched on; every other line (and
/// the file's line endings) untouched. Adds the keys if they're missing.
pub fn sims4_enable_mods(options_ini: &str) -> String {
    let nl = if options_ini.contains("\r\n") { "\r\n" } else { "\n" };
    let (mut saw_mods, mut saw_scripts) = (false, false);
    let mut out: Vec<String> = options_ini
        .lines()
        .map(|line| match line.split_once('=').map(|(k, _)| k.trim().to_ascii_lowercase()) {
            Some(k) if k == "modsdisabled" => {
                saw_mods = true;
                "modsdisabled = 0".to_string()
            }
            Some(k) if k == "scriptmodsenabled" => {
                saw_scripts = true;
                "scriptmodsenabled = 1".to_string()
            }
            _ => line.to_string(),
        })
        .collect();
    // Keys belong in the [options] section, which is the file's only section.
    let insert_at = out.iter().position(|l| l.trim().eq_ignore_ascii_case("[options]")).map_or(out.len(), |i| i + 1);
    if !saw_scripts {
        out.insert(insert_at, "scriptmodsenabled = 1".into());
    }
    if !saw_mods {
        out.insert(insert_at, "modsdisabled = 0".into());
    }
    let mut s = out.join(nl);
    if options_ini.ends_with('\n') {
        s.push_str(nl);
    }
    s
}

/// Sims 4 checks. `options_ini` is the file's text (None if it doesn't exist
/// yet, i.e. the game never ran); `zip_has_package` peeks inside an archive.
pub fn check_sims4(manifest: &FileManifest, options_ini: Option<&str>, zip_has_package: impl Fn(&str) -> bool) -> Vec<CompatIssue> {
    let mut out = Vec::new();
    let live: Vec<&String> = manifest.files.keys().filter(|p| p.starts_with("Mods/") && !is_disabled(p)).collect();
    if live.is_empty() {
        return out;
    }
    let scripts = live.iter().any(|p| ext(p) == "ts4script");

    if let Some(ini) = options_ini {
        let (mods_disabled, scripts_enabled) = sims4_mod_flags(ini);
        let mods_off = mods_disabled == Some(true);
        let scripts_off = scripts && scripts_enabled != Some(true);
        if mods_off || scripts_off {
            let title = if mods_off { "Mods are turned off in The Sims 4's options" } else { "Script mods are turned off in The Sims 4's options" };
            let mut i = issue(
                "sims4_mods_disabled",
                "error",
                title.into(),
                "The game switches mods off after most updates. Turn on Game Options → Other → \"Enable Custom Content and Mods\" and \"Script Mods Allowed\", or let SyncCrate change Options.ini for you (with the game closed), then restart the game.",
                vec![],
            );
            i.fix = Some(FIX_SIMS4_ENABLE_MODS.into());
            out.push(i);
        }
    }

    let too_deep_scripts: Vec<String> = live.iter().filter(|p| ext(p) == "ts4script" && depth_under("Mods", p).is_some_and(|d| d > 1)).map(|p| p.to_string()).collect();
    if !too_deep_scripts.is_empty() {
        out.push(issue(
            "sims4_script_too_deep",
            "error",
            format!("{} script mod{} too deep to load", too_deep_scripts.len(), if too_deep_scripts.len() == 1 { " is" } else { "s are" }),
            "The Sims 4 only loads .ts4script files directly in Mods or one folder below it. Move these up a level.",
            too_deep_scripts,
        ));
    }

    let too_deep_packages: Vec<String> = live.iter().filter(|p| ext(p) == "package" && depth_under("Mods", p).is_some_and(|d| d > 5)).map(|p| p.to_string()).collect();
    if !too_deep_packages.is_empty() {
        out.push(issue(
            "sims4_package_too_deep",
            "error",
            format!("{} package file{} too deep to load", too_deep_packages.len(), if too_deep_packages.len() == 1 { " is" } else { "s are" }),
            "The Sims 4 only loads .package files up to five folders deep inside Mods. Move these closer to the Mods folder.",
            too_deep_packages,
        ));
    }

    let packed: Vec<String> = live.iter().filter(|p| ext(p) == "zip").take(MAX_ZIPS_INSPECTED).filter(|p| zip_has_package(p)).map(|p| p.to_string()).collect();
    if !packed.is_empty() {
        out.push(issue(
            "sims4_zipped_packages",
            "warn",
            format!("{} .zip file{} still packed", packed.len(), if packed.len() == 1 { " has" } else { "s have" }),
            "These archives contain .package files, which the game never loads from inside a .zip. Extract them into Mods (and remove the .zip).",
            packed,
        ));
    }
    out
}

/// Whether the archive at `path` has a `.package` entry. Unreadable → false.
pub fn zip_contains_package(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return false };
    if !meta.file_type().is_file() {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else { return false };
    let Ok(zip) = zip::ZipArchive::new(file) else { return false };
    let found = zip.file_names().any(|n| n.to_ascii_lowercase().ends_with(".package"));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FileInfo;

    fn manifest(paths: &[&str]) -> FileManifest {
        let mut m = FileManifest::default();
        for p in paths {
            let ft = if p.ends_with(".ts4script") { "Mod" } else { "CustomContent" };
            m.files.insert(p.to_string(), FileInfo { relative_path: p.to_string(), size: 1, hash: String::new(), modified: 0, file_type: ft.into() });
        }
        m
    }

    fn registry_game(id: &str) -> GameDefinition {
        crate::registry::load_registry().games.into_iter().find(|g| g.id == id).unwrap()
    }

    #[test]
    fn missing_loader_only_when_mods_exist_and_loader_files_dont() {
        let valheim = registry_game("valheim");
        assert!(valheim.mod_loader.is_some(), "registry declares BepInEx for Valheim");
        let m = manifest(&["BepInEx/plugins/Cool/Cool.dll", "BepInEx/plugins/Off.dll.disabled"]);
        let issue = check_loader(&valheim, &m, |_| false).expect("no core → issue");
        assert_eq!(issue.count, 1, "disabled plugins don't count");
        assert!(issue.url.is_some());
        assert!(check_loader(&valheim, &m, |p| p == "BepInEx/core/BepInEx.dll").is_none());
        assert!(check_loader(&valheim, &manifest(&[]), |_| false).is_none(), "no mods, nothing to warn about");
        // Config files alone aren't mods.
        assert!(check_loader(&valheim, &manifest(&["BepInEx/config/x.cfg"]), |_| false).is_none());
        let stardew = registry_game("stardew_valley");
        assert!(check_loader(&stardew, &manifest(&["Mods/LookupAnything/LookupAnything.dll"]), |_| false).is_some());
    }

    #[test]
    fn options_ini_flags_and_fix_keep_everything_else() {
        let ini = "[options]\r\nversion = 5\r\nmodsdisabled = 1\r\nscriptmodsenabled = 0\r\nfullscreen = 1\r\n";
        assert_eq!(sims4_mod_flags(ini), (Some(true), Some(false)));
        let fixed = sims4_enable_mods(ini);
        assert_eq!(fixed, "[options]\r\nversion = 5\r\nmodsdisabled = 0\r\nscriptmodsenabled = 1\r\nfullscreen = 1\r\n");
        assert_eq!(sims4_mod_flags(&fixed), (Some(false), Some(true)));
        // Missing keys are added inside [options].
        let fixed = sims4_enable_mods("[options]\nversion = 5\n");
        assert_eq!(fixed, "[options]\nmodsdisabled = 0\nscriptmodsenabled = 1\nversion = 5\n");
    }

    #[test]
    fn sims4_checks() {
        let m = manifest(&[
            "Mods/ok.package",
            "Mods/Creator/ok.ts4script",
            "Mods/A/B/deep.ts4script",
            "Mods/A/B/Off.ts4script.disabled",
            "Mods/1/2/3/4/5/fine.package",
            "Mods/1/2/3/4/5/6/deep.package",
            "Mods/cc.zip",
            "Mods/script.zip",
        ]);
        let off = "[options]\nmodsdisabled = 1\nscriptmodsenabled = 1\n";
        let issues = check_sims4(&m, Some(off), |p| p == "Mods/cc.zip");
        let kinds: Vec<_> = issues.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, vec!["sims4_mods_disabled", "sims4_script_too_deep", "sims4_package_too_deep", "sims4_zipped_packages"]);
        assert_eq!(issues[0].fix.as_deref(), Some(FIX_SIMS4_ENABLE_MODS));
        assert_eq!(issues[1].paths, vec!["Mods/A/B/deep.ts4script".to_string()], "disabled scripts don't count");
        assert_eq!(issues[2].paths, vec!["Mods/1/2/3/4/5/6/deep.package".to_string()]);
        assert_eq!(issues[3].paths, vec!["Mods/cc.zip".to_string()]);

        // All good: nothing reported. Scripts need scriptmodsenabled; packages alone don't.
        let on = "[options]\nmodsdisabled = 0\nscriptmodsenabled = 1\n";
        assert!(check_sims4(&manifest(&["Mods/a.package", "Mods/x.ts4script"]), Some(on), |_| false).is_empty());
        let no_scripts = "[options]\nmodsdisabled = 0\nscriptmodsenabled = 0\n";
        assert!(check_sims4(&manifest(&["Mods/a.package"]), Some(no_scripts), |_| false).is_empty());
        assert_eq!(check_sims4(&manifest(&["Mods/x.ts4script"]), Some(no_scripts), |_| false)[0].title, "Script mods are turned off in The Sims 4's options");
        // No mods at all, or no Options.ini yet: no options warning.
        assert!(check_sims4(&manifest(&["Saves/a.save"]), Some(off), |_| false).is_empty());
        assert!(check_sims4(&manifest(&["Mods/a.package"]), None, |_| false).is_empty());
    }

    #[test]
    fn zip_inspection() {
        use std::io::Write;
        let dir = crate::testutil::temp_dir("compat-zip");
        let make = |name: &str, entry: &str| {
            let p = dir.join(name);
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            z.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
            p
        };
        assert!(zip_contains_package(&make("cc.zip", "Folder/Hair.package")));
        assert!(!zip_contains_package(&make("script.zip", "mod/__init__.pyc")));
        std::fs::write(dir.join("bad.zip"), b"nope").unwrap();
        assert!(!zip_contains_package(&dir.join("bad.zip")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
