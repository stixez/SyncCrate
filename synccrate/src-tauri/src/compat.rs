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
//! - Paradox games: `mod/<name>.mod` descriptors whose `path=` is an
//!   absolute path. The launcher writes those, and once synced they point at
//!   a folder on the host's PC; `path="mod/<folder>"` works everywhere.
use crate::registry::GameDefinition;
use crate::state::FileManifest;
use serde::Serialize;
use std::path::Path;

/// Paths listed per issue; the count says how many there really are.
const MAX_PATHS: usize = 50;
const MAX_ZIPS_INSPECTED: usize = 500;
pub const FIX_SIMS4_ENABLE_MODS: &str = "sims4_enable_mods";
pub const FIX_PARADOX_RELATIVE_PATHS: &str = "paradox_relative_mod_paths";
/// Games whose launcher finds local mods through `mod/<name>.mod`
/// descriptors with a `path=` line (Victoria 3 uses metadata.json instead).
pub const PARADOX_DESCRIPTOR_GAMES: &[&str] = &["crusader_kings_3", "europa_universalis_4", "hearts_of_iron_4", "stellaris"];
/// Descriptors are a few lines; anything bigger isn't one.
pub const MAX_DESCRIPTOR_BYTES: u64 = 64 * 1024;

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

/// The `path="..."` value of a Paradox descriptor, and the line it's on.
fn descriptor_path_line(text: &str) -> Option<(usize, String)> {
    text.lines().enumerate().find_map(|(i, line)| {
        let rest = line.trim_start().strip_prefix("path")?.trim_start().strip_prefix('=')?.trim();
        let value = rest.trim_matches('"');
        (!value.is_empty()).then(|| (i, value.to_string()))
    })
}

/// A path the launcher wrote for this PC: `C:/...`, `C:\...`, `/home/...`, `~/...`.
fn is_absolute_mod_path(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || p.starts_with('\\') || p.starts_with('~') || (b.len() > 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
}

/// The mod folder a descriptor path ends in.
fn mod_folder_name(p: &str) -> Option<&str> {
    p.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|s| !s.is_empty() && *s != "." && *s != "..")
}

/// Where a descriptor's absolute path leads, as seen by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorTarget {
    /// This game's own `mod/<folder>`: fixable here.
    ThisModFolder,
    /// Nothing on this PC: the descriptor came from someone else's PC.
    Missing,
    /// Some other folder that exists (a dev copy elsewhere): left alone.
    Elsewhere,
}

/// Descriptors (`mod/<name>.mod`) whose path is absolute, split into the
/// ones pointing at this game's `mod/<folder>` (fixable here) and ones that
/// came from someone else's PC. Only descriptors whose mod folder is really
/// in `mod/` here count; `(descriptor, folder as spelled on disk)` pairs.
/// `target(path, folder)` says where the path leads.
pub fn absolute_descriptors(
    manifest: &FileManifest,
    read: impl Fn(&str) -> Option<String>,
    target: impl Fn(&str, &str) -> DescriptorTarget,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let (mut own, mut foreign) = (Vec::new(), Vec::new());
    for key in manifest.files.keys() {
        let lower = key.to_ascii_lowercase();
        let Some(name) = lower.strip_prefix("mod/") else { continue };
        if name.contains('/') || !name.ends_with(".mod") || name.starts_with("ugc_") {
            continue;
        }
        let Some((_, path)) = read(key).as_deref().and_then(descriptor_path_line) else { continue };
        if !is_absolute_mod_path(&path) {
            continue;
        }
        let Some(folder) = mod_folder_name(&path) else { continue };
        let prefix = format!("mod/{}/", folder.to_ascii_lowercase());
        // The folder name as it's spelled here (it matters on Linux).
        let Some(on_disk) = manifest.files.keys().find(|k| k.to_ascii_lowercase().starts_with(&prefix)).and_then(|k| k.get(4..4 + folder.len())) else {
            continue; // the mod itself isn't here: a different problem
        };
        let entry = (key.clone(), on_disk.to_string());
        match target(&path, on_disk) {
            DescriptorTarget::ThisModFolder => own.push(entry),
            DescriptorTarget::Missing => foreign.push(entry),
            DescriptorTarget::Elsewhere => {}
        }
    }
    own.sort();
    foreign.sort();
    (own, foreign)
}

/// The descriptor with its `path=` line changed to `path="mod/<folder>"`,
/// everything else (line endings included) untouched.
pub fn relative_descriptor(text: &str, folder: &str) -> String {
    let Some((idx, _)) = descriptor_path_line(text) else { return text.to_string() };
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == idx {
            let indent = &line[..line.len() - line.trim_start().len()];
            let ending = if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" };
            out.push_str(&format!("{indent}path=\"mod/{folder}\"{ending}"));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Paradox checks: descriptors with absolute paths, here or from another PC.
pub fn check_paradox(manifest: &FileManifest, read: impl Fn(&str) -> Option<String>, target: impl Fn(&str, &str) -> DescriptorTarget) -> Vec<CompatIssue> {
    let (own, foreign) = absolute_descriptors(manifest, read, target);
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let mut out = Vec::new();
    if !own.is_empty() {
        let n = own.len();
        let mut i = issue(
            "paradox_absolute_paths",
            "warn",
            format!("{n} mod{} only work on this PC", plural(n)),
            "Their descriptors point at the mod folder by its full path on this PC, so friends who sync them get a path that doesn't exist on their PC. SyncCrate can switch them to path=\"mod/<folder>\", which works on every PC. The old versions stay in File history.",
            own.into_iter().map(|(d, _)| d).collect(),
        );
        i.fix = Some(FIX_PARADOX_RELATIVE_PATHS.into());
        out.push(i);
    }
    if !foreign.is_empty() {
        let n = foreign.len();
        out.push(issue(
            "paradox_foreign_paths",
            "warn",
            format!("{n} mod{} point at another PC's folder", plural(n)),
            "These descriptors came with a full path to a folder on someone else's PC, so the launcher can't find the mods. Ask whoever hosts to run this health check and let SyncCrate fix their mod paths, then sync again.",
            foreign.into_iter().map(|(d, _)| d).collect(),
        ));
    }
    out
}

/// Whether the archive at `path` has a `.package` entry. Unreadable → false.
pub fn zip_contains_package(path: &Path) -> bool {
    // Same bounds as mod metadata: the archive may come from a friend.
    let Some(zip) = crate::mod_meta::open_bounded_zip(path, 4 * 1024 * 1024 * 1024) else { return false };
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

    #[test]
    fn paradox_descriptors_with_absolute_paths() {
        let m = manifest(&[
            "mod/Mine.mod", "mod/Mine/descriptor.mod", "mod/Mine/common/x.txt",
            "mod/Theirs.mod", "mod/Theirs/descriptor.mod",
            "mod/Portable.mod", "mod/Portable/descriptor.mod",
            "mod/Gone.mod",
            "mod/ugc_123.mod",
        ]);
        let files: std::collections::HashMap<&str, &str> = [
            ("mod/Mine.mod", "name=\"Mine\"\r\npath=\"C:/Users/me/Documents/Paradox Interactive/Hearts of Iron IV/mod/Mine\"\r\nsupported_version=\"1.14.*\"\r\n"),
            ("mod/Theirs.mod", "name=\"Theirs\"\npath = \"C:/Users/alex/Documents/Paradox Interactive/Hearts of Iron IV/mod/Theirs\"\n"),
            ("mod/Portable.mod", "name=\"Portable\"\npath=\"mod/Portable\"\n"),
            ("mod/Gone.mod", "path=\"C:/Users/me/Documents/Paradox Interactive/Hearts of Iron IV/mod/Gone\"\n"),
            ("mod/ugc_123.mod", "path=\"D:/Steam/steamapps/workshop/content/394360/123\"\n"),
        ]
        .into_iter()
        .collect();
        let read = |k: &str| files.get(k).map(|s| s.to_string());
        let here = |p: &str, _folder: &str| if p.contains("/Users/me/") { DescriptorTarget::ThisModFolder } else { DescriptorTarget::Missing };
        let (own, foreign) = absolute_descriptors(&m, read, here);
        assert_eq!(own, vec![("mod/Mine.mod".to_string(), "Mine".to_string())]);
        assert_eq!(foreign, vec![("mod/Theirs.mod".to_string(), "Theirs".to_string())], "Gone has no folder here, ugc is the Workshop's");

        let kinds: Vec<String> = check_paradox(&m, read, here).into_iter().map(|i| i.kind).collect();
        assert_eq!(kinds, ["paradox_absolute_paths", "paradox_foreign_paths"]);

        let fixed = relative_descriptor(files["mod/Mine.mod"], "Mine");
        assert_eq!(fixed, "name=\"Mine\"\r\npath=\"mod/Mine\"\r\nsupported_version=\"1.14.*\"\r\n");
        assert_eq!(relative_descriptor("name=\"x\"\n", "x"), "name=\"x\"\n", "no path line: unchanged");
        assert!(is_absolute_mod_path("C:\\Users\\a\\mod\\x") && is_absolute_mod_path("/home/a/.local/share/x") && !is_absolute_mod_path("mod/x"));
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
