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
//!   they need extracting). Also: two copies of one script mod (same Python
//!   modules), a missing or empty `Resource.cfg` (it decides which folders
//!   load), Tray items saved into Mods and CC saved into Tray (neither is
//!   read there), and script mods named in the game's own error reports.
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
pub const FIX_SIMS4_RESOURCE_CFG: &str = "sims4_resource_cfg";
pub const FIX_SIMS4_DISABLE_OLDER_SCRIPTS: &str = "sims4_disable_older_scripts";
pub const FIX_SIMS4_MOVE_TRAY_FILES: &str = "sims4_move_tray_files";
pub const FIX_SIMS4_MOVE_CC_TO_MODS: &str = "sims4_move_cc_to_mods";
pub const FIX_SIMS4_CLEAR_REPORTS: &str = "sims4_clear_error_reports";

/// The Resource.cfg the game writes into a new Mods folder (CRLF like the
/// game's). Each line is one folder level, so it loads `.package` files up
/// to five folders deep.
pub const SIMS4_RESOURCE_CFG: &str = "Priority 500\r\nPackedFile *.package\r\nPackedFile */*.package\r\nPackedFile */*/*.package\r\nPackedFile */*/*/*.package\r\nPackedFile */*/*/*/*.package\r\nPackedFile */*/*/*/*/*.package\r\n";

/// Saved households, lots and rooms (the gallery's files). The game only
/// reads them from Tray. `.bpi` (a lot's image) is handled separately: it's
/// also in the Mods content type, so only one next to real Tray files counts.
pub const SIMS4_TRAY_EXTENSIONS: &[&str] = &["trayitem", "blueprint", "householdbinary", "hhi", "sgi", "room", "rmi"];
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
    /// The fix button's text when "Fix it" doesn't say what happens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_label: Option<String>,
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
    CompatIssue { kind: kind.into(), severity: severity.into(), title, detail: detail.into(), count, paths, fix: None, fix_label: None, url: None }
}

fn with_fix(mut i: CompatIssue, fix: &str, label: &str) -> CompatIssue {
    i.fix = Some(fix.into());
    i.fix_label = Some(label.into());
    i
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
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

/// What the Sims 4 checks read besides the manifest (the caller does the IO).
#[derive(Debug, Default)]
pub struct Sims4Env {
    /// Options.ini's text; None if it doesn't exist yet (the game never ran).
    pub options_ini: Option<String>,
    /// Mods/Resource.cfg's text, if it could be read.
    pub resource_cfg: Option<String>,
    /// Mods/Resource.cfg doesn't exist (unreadable is not "missing").
    pub resource_cfg_missing: bool,
    /// Live script mods and the Python modules inside them.
    pub scripts: Vec<ScriptModules>,
    /// Files under Mods with Tray extensions (the scan doesn't list them).
    pub tray_in_mods: Vec<String>,
    /// The game's error reports (lastException*.txt), newest first.
    pub reports: Vec<ErrorReport>,
    /// When the game was last updated (reports from before it are old news).
    pub patch_time: Option<u64>,
    /// Connected to a host: files the host syncs mustn't be moved here, the
    /// next sync would just bring them back.
    pub is_client: bool,
    /// The host's file list has a Mods/Resource.cfg (client only).
    pub host_has_resource_cfg: bool,
}

#[derive(Debug, Clone)]
pub struct ScriptModules {
    pub path: String,
    pub modified: u64,
    pub modules: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct ErrorReport {
    pub modified: u64,
    pub text: String,
}

/// Top-level Python modules in a `.ts4script` (a zip): `mc_cmd_center/x.pyc`
/// → `mc_cmd_center`, a root `tool.pyc` → `tool`. A folder without its own
/// `__init__` is a namespace one creator shares between mods
/// (`creator/mod_a/...`), so it counts one level deeper: keyed by the top
/// folder alone, two different mods of theirs looked like one mod twice.
pub fn script_module_names<'a>(entries: impl Iterator<Item = &'a str>) -> std::collections::BTreeSet<String> {
    let py: Vec<String> = entries
        .map(|e| e.to_ascii_lowercase().replace('\\', "/"))
        .filter(|e| e.ends_with(".pyc") || e.ends_with(".py"))
        .collect();
    let packages: std::collections::HashSet<&str> = py
        .iter()
        .filter_map(|e| e.strip_suffix("/__init__.pyc").or_else(|| e.strip_suffix("/__init__.py")))
        .filter(|top| !top.contains('/'))
        .collect();
    py.iter()
        .filter_map(|e| {
            let parts: Vec<&str> = e.split('/').collect();
            let key = match parts.as_slice() {
                [file] => file.strip_suffix(".pyc").or_else(|| file.strip_suffix(".py")).unwrap_or(file).to_string(),
                [top, _] => top.to_string(),
                [top, sub, ..] if !packages.contains(top) => format!("{top}/{sub}"),
                [top, ..] => top.to_string(),
                [] => return None,
            };
            (!key.is_empty() && !key.starts_with("__")).then_some(key)
        })
        .collect()
}

/// Script mods holding exactly the same modules: two versions of one mod,
/// which the game both runs. Each group newest first. (Same file name alone
/// isn't enough: different creators' `main.ts4script` are different mods,
/// and a renamed old version still conflicts.)
///
/// Only copies the game loads (at most one folder deep) take part: with a
/// newer copy too deep to load, "turn off the older" turned off the one
/// that worked. The too-deep one is reported on its own.
pub fn duplicate_scripts(scripts: &[ScriptModules]) -> Vec<Vec<&ScriptModules>> {
    let mut by_modules: std::collections::BTreeMap<&std::collections::BTreeSet<String>, Vec<&ScriptModules>> = Default::default();
    for s in scripts.iter().filter(|s| !s.modules.is_empty() && depth_under("Mods", &s.path).is_some_and(|d| d <= 1)) {
        by_modules.entry(&s.modules).or_default().push(s);
    }
    by_modules
        .into_values()
        .filter(|g| g.len() > 1)
        .map(|mut g| {
            g.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
            g
        })
        .collect()
}

/// All but the newest copy of each duplicated script mod.
pub fn older_script_copies(scripts: &[ScriptModules]) -> Vec<String> {
    duplicate_scripts(scripts).into_iter().flat_map(|g| g.into_iter().skip(1).map(|s| s.path.clone())).collect()
}

/// The `PackedFile` patterns of a Resource.cfg, split into lowercase path parts.
fn packed_file_patterns(cfg: &str) -> Vec<Vec<String>> {
    cfg.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (kw, rest) = line.split_once(char::is_whitespace)?;
            if !kw.eq_ignore_ascii_case("packedfile") {
                return None;
            }
            let pat = rest.trim().trim_matches('"').replace('\\', "/").to_ascii_lowercase();
            (!pat.is_empty()).then(|| pat.split('/').map(str::to_string).collect())
        })
        .collect()
}

/// `*` matches any run of characters inside one path part.
fn wildcard(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == s;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s[first.len()..].ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// Whether a Resource.cfg with these patterns loads `rel` (a path under Mods).
fn cfg_loads(patterns: &[Vec<String>], rel: &str) -> bool {
    let parts: Vec<String> = rel.to_ascii_lowercase().split('/').map(str::to_string).collect();
    patterns.iter().any(|p| p.len() == parts.len() && p.iter().zip(&parts).all(|(a, b)| wildcard(a, b)))
}

/// Tray files among `found` (paths under Mods with a Tray or `.bpi`
/// extension): a `.bpi` counts only in a folder that also has Tray files.
pub fn tray_files(found: &[String]) -> Vec<String> {
    let parent = |p: &str| p.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_ascii_lowercase());
    let tray_dirs: std::collections::HashSet<String> =
        found.iter().filter(|p| SIMS4_TRAY_EXTENSIONS.contains(&ext(p).as_str())).map(|p| parent(p)).collect();
    let mut out: Vec<String> = found
        .iter()
        .filter(|p| {
            let e = ext(p);
            SIMS4_TRAY_EXTENSIONS.contains(&e.as_str()) || (e == "bpi" && tray_dirs.contains(&parent(p)))
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// CC saved into Tray (the game only loads it from Mods).
pub fn cc_in_tray(manifest: &FileManifest) -> Vec<String> {
    let mut out: Vec<String> = manifest
        .files
        .keys()
        .filter(|p| p.starts_with("Tray/") && !is_disabled(p) && matches!(ext(p).as_str(), "package" | "ts4script"))
        .cloned()
        .collect();
    out.sort();
    out
}

/// The script mods a (lowercase) report mentions: by path under Mods where
/// it shows one (tracebacks do), else by bare file name. One pass per
/// report: searching it once per installed script was thousands of passes
/// over up to 4 MB.
#[derive(Default)]
struct ReportNames {
    /// `folder/name.ts4script`, relative to Mods.
    paths: std::collections::HashSet<String>,
    /// Names seen without a Mods path, with every tail after a space or
    /// slash (the name's start isn't marked in running text).
    bare: std::collections::HashSet<String>,
}

fn script_names_in(text: &str) -> ReportNames {
    const EXT: &str = ".ts4script";
    let bytes = text.as_bytes();
    let mut out = ReportNames::default();
    for (i, _) in text.match_indices(EXT) {
        let start = bytes[..i].iter().rposition(|b| matches!(b, b'"' | b'\'' | b'\n' | b'\r' | b'\t' | b'>' | b'<' | b'(')).map_or(0, |p| p + 1);
        let token = text[start..i + EXT.len()].replace('\\', "/");
        // The game's own Mods folder first: a mod may have a "mods" folder too.
        let under = token.rfind("/the sims 4/mods/").map(|p| p + "/the sims 4/mods/".len()).or_else(|| token.find("/mods/").map(|p| p + "/mods/".len()));
        match under {
            Some(p) => {
                out.paths.insert(token[p..].to_string());
            }
            None => {
                out.bare.insert(token.trim().to_string());
                for (j, c) in token.char_indices() {
                    if c == ' ' || c == '/' {
                        out.bare.insert(token[j + 1..].to_string());
                    }
                }
            }
        }
    }
    out
}

/// Live script mods named in an error report written since the last game
/// update and not updated since that report.
pub fn scripts_in_reports(manifest: &FileManifest, reports: &[ErrorReport], patch_time: Option<u64>) -> Vec<String> {
    let recent: Vec<(u64, ReportNames)> =
        reports.iter().filter(|r| r.modified > patch_time.unwrap_or(0)).map(|r| (r.modified, script_names_in(&r.text.to_ascii_lowercase()))).collect();
    if recent.is_empty() {
        return Vec::new();
    }
    let scripts: Vec<&crate::state::FileInfo> = manifest
        .files
        .values()
        .filter(|f| f.relative_path.starts_with("Mods/") && !is_disabled(&f.relative_path) && ext(&f.relative_path) == "ts4script")
        .collect();
    let name_of = |f: &crate::state::FileInfo| f.relative_path.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    let mut name_count: std::collections::HashMap<String, usize> = Default::default();
    for f in &scripts {
        *name_count.entry(name_of(f)).or_default() += 1;
    }
    let mut out: Vec<String> = scripts
        .iter()
        .filter(|f| {
            let rel = f.relative_path["Mods/".len()..].to_ascii_lowercase();
            let name = name_of(f);
            // A bare name only counts when one installed script has it:
            // every creator's `main.ts4script` isn't broken because one is.
            recent.iter().any(|(when, n)| f.modified < *when && (n.paths.contains(&rel) || (name_count[&name] == 1 && n.bare.contains(&name))))
        })
        .map(|f| f.relative_path.clone())
        .collect();
    out.sort();
    out
}

/// Sims 4 checks; `zip_has_package` peeks inside an archive.
pub fn check_sims4(manifest: &FileManifest, env: &Sims4Env, zip_has_package: impl Fn(&str) -> bool) -> Vec<CompatIssue> {
    let mut out = Vec::new();
    let tray_cc = cc_in_tray(manifest);
    let live: Vec<&String> = manifest.files.keys().filter(|p| p.starts_with("Mods/") && !is_disabled(p)).collect();
    if live.is_empty() && tray_cc.is_empty() && env.tray_in_mods.is_empty() {
        return out;
    }
    let scripts = live.iter().any(|p| ext(p) == "ts4script");

    if let Some(ini) = env.options_ini.as_deref().filter(|_| !live.is_empty()) {
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

    let packages: Vec<&String> = live.iter().copied().filter(|p| ext(p) == "package").collect();
    let in_subfolders = packages.iter().any(|p| depth_under("Mods", p).is_some_and(|d| d > 0));
    let cfg_patterns = env.resource_cfg.as_deref().map(packed_file_patterns);
    let cfg_empty = cfg_patterns.as_ref().is_some_and(|p| p.is_empty());
    // The file syncs: a client whose host has one gets the host's, so a
    // local fix would just turn into a conflict with it.
    let from_host = env.is_client && env.host_has_resource_cfg;
    let cfg_fix = |i: CompatIssue, label: &str, advice: &str| {
        if from_host {
            let mut i = i;
            i.detail.push(' ');
            i.detail.push_str(advice);
            i
        } else {
            with_fix(i, FIX_SIMS4_RESOURCE_CFG, label)
        }
    };
    if cfg_empty && !packages.is_empty() {
        out.push(cfg_fix(
            issue(
                "sims4_resource_cfg_broken",
                "error",
                "Resource.cfg doesn't load any CC".into(),
                "Mods/Resource.cfg tells the game which folders to load .package files from, and this one lists none, so no CC loads. SyncCrate can put the standard one back (the old file is kept as a .synccrate-backup copy).",
                vec![],
            ),
            "Restore it",
            "This one comes from your host: ask them to fix theirs.",
        ));
    } else if env.resource_cfg_missing && env.options_ini.is_some() && in_subfolders {
        // Only once the game has run (Options.ini exists): a new Mods folder
        // gets its Resource.cfg on the first start.
        out.push(cfg_fix(
            issue(
                "sims4_resource_cfg_missing",
                "warn",
                "Resource.cfg is missing from Mods".into(),
                "It tells the game to load .package files from folders inside Mods. The game usually writes a new one when it starts, but until then CC in subfolders doesn't load. SyncCrate can create the standard one.",
                vec![],
            ),
            "Create it",
            "Your host has one: sync to get it.",
        ));
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

    let dupes = duplicate_scripts(&env.scripts);
    if !dupes.is_empty() {
        let paths: Vec<String> = dupes.iter().flatten().map(|s| s.path.clone()).collect();
        out.push(with_fix(
            issue(
                "sims4_duplicate_scripts",
                "error",
                if dupes.len() == 1 { "A script mod is installed twice".into() } else { format!("{} script mods are installed twice", dupes.len()) },
                "These files hold the same script mod, so the game runs two versions of it at once, which usually breaks it. Keep the newest: SyncCrate turns the older copies off (you can turn them back on in the list).",
                paths,
            ),
            FIX_SIMS4_DISABLE_OLDER_SCRIPTS,
            "Turn off older copies",
        ));
    }

    // The standard Resource.cfg (five folders) when there's none: the game
    // writes that one. An empty one was reported above instead of flagging
    // every package as too deep.
    let patterns = match cfg_patterns {
        Some(p) if !p.is_empty() => p,
        _ => packed_file_patterns(SIMS4_RESOURCE_CFG),
    };
    let too_deep_packages: Vec<String> = if cfg_empty {
        Vec::new()
    } else {
        packages.iter().filter(|p| p.strip_prefix("Mods/").is_some_and(|rel| !cfg_loads(&patterns, rel))).map(|p| p.to_string()).collect()
    };
    if !too_deep_packages.is_empty() {
        out.push(issue(
            "sims4_package_too_deep",
            "error",
            format!("{} package file{} too deep to load", too_deep_packages.len(), if too_deep_packages.len() == 1 { " is" } else { "s are" }),
            "The Sims 4 only loads .package files as deep inside Mods as Resource.cfg allows (five folders as standard). Move these closer to the Mods folder.",
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

    if !tray_cc.is_empty() {
        let mut i = issue(
            "sims4_cc_in_tray",
            "warn",
            format!("{} in the Tray folder", plural(tray_cc.len(), "CC file is", "CC files are")),
            "The game only loads .package and .ts4script files from Mods, so these do nothing in Tray.",
            tray_cc,
        );
        if env.is_client {
            i.detail.push_str(" Ask your host to move them into Mods: they come from the host, and the next sync would bring them back here.");
        } else {
            i = with_fix(i, FIX_SIMS4_MOVE_CC_TO_MODS, "Move to Mods");
        }
        out.push(i);
    }

    let tray = tray_files(&env.tray_in_mods);
    if !tray.is_empty() {
        out.push(with_fix(
            issue(
                "sims4_tray_in_mods",
                "warn",
                format!("{} in the Mods folder", plural(tray.len(), "Tray file is", "Tray files are")),
                "Saved households, lots and rooms only show up in the gallery from the Tray folder. SyncCrate can move them there.",
                tray,
            ),
            FIX_SIMS4_MOVE_TRAY_FILES,
            "Move to Tray",
        ));
    }

    let reported = scripts_in_reports(manifest, &env.reports, env.patch_time);
    if !reported.is_empty() {
        out.push(with_fix(
            issue(
                "sims4_script_errors",
                "warn",
                format!("The game's error report names {}", plural(reported.len(), "script mod", "script mods")),
                "The Sims 4 wrote an error report (lastException) since its last update that mentions these, and they haven't been updated since. Update or remove them, then delete the old reports so the next one shows only new problems. If errors keep coming, Find a broken mod narrows it down.",
                reported,
            ),
            FIX_SIMS4_CLEAR_REPORTS,
            "Delete old reports",
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
        let issues = check_sims4(&m, &env(Some(off)), |p| p == "Mods/cc.zip");
        let kinds: Vec<_> = issues.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, vec!["sims4_mods_disabled", "sims4_script_too_deep", "sims4_package_too_deep", "sims4_zipped_packages"]);
        assert_eq!(issues[0].fix.as_deref(), Some(FIX_SIMS4_ENABLE_MODS));
        assert_eq!(issues[1].paths, vec!["Mods/A/B/deep.ts4script".to_string()], "disabled scripts don't count");
        assert_eq!(issues[2].paths, vec!["Mods/1/2/3/4/5/6/deep.package".to_string()]);
        assert_eq!(issues[3].paths, vec!["Mods/cc.zip".to_string()]);

        // All good: nothing reported. Scripts need scriptmodsenabled; packages alone don't.
        let on = "[options]\nmodsdisabled = 0\nscriptmodsenabled = 1\n";
        assert!(check_sims4(&manifest(&["Mods/a.package", "Mods/x.ts4script"]), &env(Some(on)), |_| false).is_empty());
        let no_scripts = "[options]\nmodsdisabled = 0\nscriptmodsenabled = 0\n";
        assert!(check_sims4(&manifest(&["Mods/a.package"]), &env(Some(no_scripts)), |_| false).is_empty());
        assert_eq!(check_sims4(&manifest(&["Mods/x.ts4script"]), &env(Some(no_scripts)), |_| false)[0].title, "Script mods are turned off in The Sims 4's options");
        // No mods at all, or no Options.ini yet: no options warning.
        assert!(check_sims4(&manifest(&["Saves/a.save"]), &env(Some(off)), |_| false).is_empty());
        assert!(check_sims4(&manifest(&["Mods/a.package"]), &env(None), |_| false).is_empty());
    }

    fn env(ini: Option<&str>) -> Sims4Env {
        Sims4Env { options_ini: ini.map(String::from), resource_cfg: Some(SIMS4_RESOURCE_CFG.into()), ..Default::default() }
    }

    fn kinds(issues: &[CompatIssue]) -> Vec<&str> {
        issues.iter().map(|i| i.kind.as_str()).collect()
    }

    const ON: &str = "[options]\nmodsdisabled = 0\nscriptmodsenabled = 1\n";

    #[test]
    fn resource_cfg_decides_how_deep_packages_load() {
        let m = manifest(&["Mods/1/2/3/4/5/6/deep.package", "Mods/Priority/p.package", "Mods/a.package"]);
        // Standard file: six levels down is too deep.
        assert_eq!(kinds(&check_sims4(&m, &env(Some(ON)), |_| false)), ["sims4_package_too_deep"]);
        // A player who added a line for it: fine (the old fixed "five" was a false alarm).
        let mut e = env(Some(ON));
        e.resource_cfg = Some(format!("{SIMS4_RESOURCE_CFG}PackedFile */*/*/*/*/*/*.package\r\n"));
        assert!(check_sims4(&m, &e, |_| false).is_empty());
        // Named-folder lines only load that folder.
        e.resource_cfg = Some("Priority 1000\nPackedFile Priority/*.package\nPriority 500\nPackedFile *.package\n".into());
        let i = check_sims4(&m, &e, |_| false);
        assert_eq!(i[0].paths, vec!["Mods/1/2/3/4/5/6/deep.package".to_string()]);
        // No PackedFile lines at all: one clear problem, not "every package is too deep".
        e.resource_cfg = Some("Priority 500\r\n".into());
        let i = check_sims4(&m, &e, |_| false);
        assert_eq!(kinds(&i), ["sims4_resource_cfg_broken"]);
        assert_eq!(i[0].fix.as_deref(), Some(FIX_SIMS4_RESOURCE_CFG));
        // A client's file comes from the host: advice instead of a fix that
        // would turn into a conflict.
        let (mut client, mut hostless) = (Sims4Env { is_client: true, host_has_resource_cfg: true, ..env(Some(ON)) }, Sims4Env { is_client: true, ..env(Some(ON)) });
        client.resource_cfg = Some("Priority 500\r\n".into());
        hostless.resource_cfg = client.resource_cfg.clone();
        let i = check_sims4(&m, &client, |_| false);
        assert!(i[0].fix.is_none() && i[0].detail.contains("host"));
        assert!(check_sims4(&m, &hostless, |_| false)[0].fix.is_some(), "a host without one can't bring it back");
        // Missing: only once the game has run, and only if a subfolder needs it.
        let missing = |ini: Option<&str>| Sims4Env { resource_cfg_missing: true, ..env(ini) };
        let mut e = missing(Some(ON));
        e.resource_cfg = None;
        assert_eq!(kinds(&check_sims4(&manifest(&["Mods/CC/a.package"]), &e, |_| false)), ["sims4_resource_cfg_missing"]);
        assert!(check_sims4(&manifest(&["Mods/a.package"]), &e, |_| false).is_empty());
        let mut e = missing(None);
        e.resource_cfg = None;
        assert!(check_sims4(&manifest(&["Mods/CC/a.package"]), &e, |_| false).is_empty());
        assert!(wildcard("*.package", "hair.package") && wildcard("pri*ty", "priority") && !wildcard("*.package", "a.ts4script") && wildcard("*", ""));
    }

    fn script(path: &str, modified: u64, modules: &[&str]) -> ScriptModules {
        ScriptModules { path: path.into(), modified, modules: modules.iter().map(|m| m.to_string()).collect() }
    }

    #[test]
    fn same_modules_in_two_scripts_is_a_duplicate() {
        let names = script_module_names(["mc_cmd_center/__init__.pyc", "mc_cmd_center/x.pyc", "Tool.pyc", "__pycache__/a.pyc", "readme.txt", "icon.png"].into_iter());
        assert_eq!(names.into_iter().collect::<Vec<_>>(), ["mc_cmd_center", "tool"]);
        let scripts = vec![
            script("Mods/MCCC/mc_cmd_center.ts4script", 200, &["mc_cmd_center"]),
            script("Mods/Old/mc_cmd_center_old.ts4script", 100, &["mc_cmd_center"]),
            // Same file name, different mods: not duplicates.
            script("Mods/A/main.ts4script", 1, &["a_mod"]),
            script("Mods/B/main.ts4script", 2, &["b_mod"]),
            // Unreadable archives have no modules and never match.
            script("Mods/x.ts4script", 1, &[]),
            script("Mods/y.ts4script", 1, &[]),
        ];
        let groups = duplicate_scripts(&scripts);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0][0].path, "Mods/MCCC/mc_cmd_center.ts4script", "newest first");
        assert_eq!(older_script_copies(&scripts), ["Mods/Old/mc_cmd_center_old.ts4script"]);
        let e = Sims4Env { scripts, ..env(Some(ON)) };
        let i = check_sims4(&manifest(&["Mods/MCCC/mc_cmd_center.ts4script"]), &e, |_| false);
        assert_eq!(kinds(&i), ["sims4_duplicate_scripts"]);
        assert_eq!((i[0].title.as_str(), i[0].count), ("A script mod is installed twice", 2));

        // A newer copy too deep to load isn't a duplicate: turning the older
        // one off would leave none that works.
        let deep = vec![script("Mods/MCCC/mc.ts4script", 100, &["mc_cmd_center"]), script("Mods/DL/MCCC/mc.ts4script", 200, &["mc_cmd_center"])];
        assert!(duplicate_scripts(&deep).is_empty());
        // One creator's namespace folder shared by two mods: different mods.
        let a = script_module_names(["creator/mod_a/main.pyc", "creator/mod_a/x.pyc"].into_iter());
        let b = script_module_names(["creator/mod_b/main.pyc"].into_iter());
        assert_ne!(a, b);
        assert_eq!(a.into_iter().collect::<Vec<_>>(), ["creator/mod_a"]);
        // A real package (with __init__) stays one module.
        let pkg = script_module_names(["mc_cmd_center/__init__.pyc", "mc_cmd_center/sub/x.pyc"].into_iter());
        assert_eq!(pkg.into_iter().collect::<Vec<_>>(), ["mc_cmd_center"]);
    }

    #[test]
    fn tray_files_in_mods_and_cc_in_tray() {
        let found: Vec<String> = ["Mods/Lots/0x1!0xab.blueprint", "Mods/Lots/0x1!0xab.bpi", "Mods/CC/preview.bpi", "Mods/Sims/0x2!0xcd.trayitem", "Mods/Sims/0x2!0xcd.hhi"].iter().map(|s| s.to_string()).collect();
        assert_eq!(tray_files(&found), ["Mods/Lots/0x1!0xab.blueprint", "Mods/Lots/0x1!0xab.bpi", "Mods/Sims/0x2!0xcd.hhi", "Mods/Sims/0x2!0xcd.trayitem"], "a lone .bpi next to CC stays");
        let m = manifest(&["Tray/hair.package", "Tray/x.trayitem", "Tray/old.package.disabled"]);
        let e = Sims4Env { tray_in_mods: found.clone(), ..env(Some(ON)) };
        let i = check_sims4(&m, &e, |_| false);
        assert_eq!(kinds(&i), ["sims4_cc_in_tray", "sims4_tray_in_mods"], "reported even with nothing else in Mods");
        assert_eq!(i[0].paths, ["Tray/hair.package"]);
        assert_eq!(i[0].fix.as_deref(), Some(FIX_SIMS4_MOVE_CC_TO_MODS));
        assert_eq!(i[1].fix.as_deref(), Some(FIX_SIMS4_MOVE_TRAY_FILES));
        // On a client the host's files would come back: advice, no button.
        let e = Sims4Env { is_client: true, ..env(Some(ON)) };
        let i = check_sims4(&m, &e, |_| false);
        assert!(i[0].fix.is_none() && i[0].detail.contains("host"));
    }

    #[test]
    fn error_reports_name_scripts_that_were_not_updated_since() {
        let mut m = manifest(&["Mods/WW/wickedwhims.ts4script", "Mods/data.ts4script", "Mods/a.ts4script", "Mods/New/fixed.ts4script"]);
        for f in m.files.values_mut() {
            f.modified = if f.relative_path.contains("fixed") { 900 } else { 100 };
        }
        let text = r#"<report><desyncdata>File "C:\Users\me\Documents\Electronic Arts\The Sims 4\Mods\WW\wickedwhims.ts4script\wickedwhims\main.py", line 3
File "T:\Mods\data.ts4script\x.py" T:\Mods\New\FIXED.TS4SCRIPT\y.py</desyncdata></report>"#;
        let report = |modified| ErrorReport { modified, text: text.into() };
        assert_eq!(scripts_in_reports(&m, &[report(500)], Some(400)), ["Mods/WW/wickedwhims.ts4script", "Mods/data.ts4script"], "a.ts4script isn't matched inside data.ts4script; fixed was updated since");
        assert!(scripts_in_reports(&m, &[report(300)], Some(400)).is_empty(), "from before the last game update");

        // Same name in two folders: the report's path picks the right one;
        // a bare name picks neither.
        let m2 = manifest(&["Mods/A/main.ts4script", "Mods/B/main.ts4script", "Mods/solo.ts4script"]);
        let traced = ErrorReport { modified: 500, text: r#"File "C:\Users\me\Documents\Electronic Arts\The Sims 4\Mods\A\main.ts4script\a\x.py""#.into() };
        assert_eq!(scripts_in_reports(&m2, &[traced], None), ["Mods/A/main.ts4script"]);
        let bare = ErrorReport { modified: 500, text: "Error in main.ts4script and in solo.ts4script\r\n".into() };
        assert_eq!(scripts_in_reports(&m2, &[bare], None), ["Mods/solo.ts4script"]);
        let e = Sims4Env { reports: vec![report(500)], patch_time: Some(400), ..env(Some(ON)) };
        let i = check_sims4(&m, &e, |_| false);
        assert_eq!(kinds(&i), ["sims4_script_errors"]);
        assert_eq!(i[0].fix_label.as_deref(), Some("Delete old reports"));
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
