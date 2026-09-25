use crate::registry::{DetectionStrategy, GameDefinition, GameRegistry};
use std::path::PathBuf;

/// Root config directory: the real OS config dir, unless overridden (E2E
/// tests point this at a temp dir so a test run never reads or writes the
/// developer's real SyncCrate data — hash cache, sync config, backups, ...).
pub fn config_root() -> PathBuf {
    if let Ok(dir) = std::env::var("SYNCCRATE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Strip the Windows extended-length path prefix (\\?\) that canonicalize() adds.
#[allow(dead_code)]
pub fn clean_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// Scan all direct children of a directory for a given subfolder path.
/// Handles localized folder names (e.g. OneDrive/Dokumenti, OneDrive/Documenti).
#[cfg(target_os = "windows")]
fn scan_children_for(parent: &std::path::Path, sub: &std::path::Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(sub);
            if candidate.exists() {
                results.push(candidate);
            }
        }
    }
    results
}

/// Build candidate paths for a documents-relative detection strategy.
/// `base` is e.g. "Electronic Arts" or "EA Games".
/// `folders` is e.g. &["The Sims 4"].
fn build_candidates(base: &str, folders: &[String]) -> Vec<PathBuf> {
    let folder_refs: Vec<&str> = folders.iter().map(|s| s.as_str()).collect();
    build_candidates_str(base, &folder_refs)
}

fn build_candidates_str(base: &str, folders: &[&str]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. System Documents folder (handles Windows Known Folders, localized names)
    if let Some(docs) = dirs::document_dir() {
        for folder in folders {
            candidates.push(docs.join(base).join(folder));
        }
    }

    // 2. Windows-specific: scan OneDrive subfolders for localized Documents
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = dirs::home_dir() {
            let onedrive = home.join("OneDrive");
            if onedrive.exists() {
                for folder in folders {
                    let sub = PathBuf::from(base).join(folder);
                    candidates.extend(scan_children_for(&onedrive, &sub));
                }
            }
            let onedrive_personal = home.join("OneDrive - Personal");
            if onedrive_personal.exists() {
                for folder in folders {
                    let sub = PathBuf::from(base).join(folder);
                    candidates.extend(scan_children_for(&onedrive_personal, &sub));
                }
            }
        }
    }

    // 3. Linux: common alternate locations
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            for folder in folders {
                candidates.push(home.join("Documents").join(base).join(folder));
            }
        }
    }

    candidates
}

/// Detect a game path using the detection strategies from a GameDefinition.
pub fn detect_game_path_from_def(game_def: &GameDefinition) -> Option<String> {
    let detection = game_def.detection.as_ref()?;
    let accept = |p: &std::path::Path| -> bool {
        p.exists()
            && (detection.require_any.is_empty()
                || detection.require_any.iter().any(|r| p.join(r).exists()))
    };

    for strategy in &detection.strategies {
        match strategy {
            DetectionStrategy::DocumentsRelative { base, folders } => {
                let candidates = build_candidates(base, folders);
                if let Some(found) = candidates.into_iter().find(|p| accept(p)) {
                    return Some(found.to_string_lossy().to_string());
                }
            }
            DetectionStrategy::AbsolutePaths { paths } => {
                let platform_paths = get_platform_paths(paths);
                for path_str in platform_paths {
                    let expanded = expand_path_vars(path_str);
                    let path = PathBuf::from(&expanded);
                    if accept(&path) {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
            DetectionStrategy::SteamLibrary { folders } => {
                for common in steam_common_dirs() {
                    for folder in folders {
                        let path = common.join(folder);
                        if accept(&path) {
                            return Some(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
            DetectionStrategy::WindowsRegistry { keys, value, subpath } => {
                for key in keys {
                    if let Some(dir) = read_registry_string(key, value) {
                        let path = if subpath.is_empty() {
                            PathBuf::from(&dir)
                        } else {
                            PathBuf::from(&dir).join(subpath)
                        };
                        if accept(&path) {
                            return Some(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Candidate Steam root directories for this platform.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        for (key, value) in [
            (r"HKCU\Software\Valve\Steam", "SteamPath"),
            (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
            (r"HKLM\SOFTWARE\Valve\Steam", "InstallPath"),
        ] {
            if let Some(p) = read_registry_string(key, value) {
                roots.push(PathBuf::from(p.replace('/', "\\")));
            }
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(pf) = std::env::var(var) {
                roots.push(PathBuf::from(pf).join("Steam"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Library/Application Support/Steam"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".steam/steam"));
            roots.push(home.join(".local/share/Steam"));
            roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        }
    }

    let mut unique: Vec<PathBuf> = Vec::new();
    for r in roots {
        let key = r.to_string_lossy().to_lowercase();
        if r.exists() && !unique.iter().any(|u| u.to_string_lossy().to_lowercase() == key) {
            unique.push(r);
        }
    }
    unique
}

/// Extract library paths from the contents of a Steam `libraryfolders.vdf`.
/// Handles both the modern (`"path" "D:\\SteamLibrary"`) and legacy
/// (`"1" "D:\\SteamLibrary"`) formats.
pub(crate) fn parse_steam_library_vdf(contents: &str) -> Vec<PathBuf> {
    let mut libs = Vec::new();
    for line in contents.lines() {
        let tokens: Vec<&str> = line.split('"').collect();
        // A key/value line splits into: ["", key, "\t\t", value, ""]
        if tokens.len() < 5 {
            continue;
        }
        let key = tokens[1];
        let value = tokens[3];
        let is_path_key = key == "path" || (!key.is_empty() && key.chars().all(|c| c.is_ascii_digit()));
        if is_path_key && (value.contains('\\') || value.contains('/')) {
            libs.push(PathBuf::from(value.replace("\\\\", "\\")));
        }
    }
    libs
}

/// All `steamapps/common` directories across every Steam library on this machine.
pub fn steam_common_dirs() -> Vec<PathBuf> {
    steam_steamapps_dirs()
        .into_iter()
        .map(|s| s.join("common"))
        .filter(|c| c.exists())
        .collect()
}

/// All `steamapps` directories (where `appmanifest_<id>.acf` files live)
/// across every Steam library on this machine.
pub fn steam_steamapps_dirs() -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in steam_roots() {
        libs.push(root.clone());
        for vdf in [
            root.join("steamapps").join("libraryfolders.vdf"),
            root.join("config").join("libraryfolders.vdf"),
        ] {
            if let Ok(contents) = std::fs::read_to_string(&vdf) {
                libs.extend(parse_steam_library_vdf(&contents));
            }
        }
    }

    let mut result: Vec<PathBuf> = Vec::new();
    for lib in libs {
        let steamapps = lib.join("steamapps");
        let key = steamapps.to_string_lossy().to_lowercase();
        if steamapps.exists() && !result.iter().any(|r| r.to_string_lossy().to_lowercase() == key) {
            result.push(steamapps);
        }
    }
    result
}

/// Read a REG_SZ / REG_EXPAND_SZ value (`key` like `HKLM\SOFTWARE\...`),
/// checking both the 64-bit and 32-bit views. Native API rather than
/// `reg query`: spawning reg.exe cost ~100 ms per call, and install detection
/// makes dozens, and its OEM-code-page output mangled non-ASCII paths.
#[cfg(target_os = "windows")]
pub fn read_registry_string(key: &str, value: &str) -> Option<String> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};
    const KEY_WOW64_64KEY: u32 = 0x0100;
    const KEY_WOW64_32KEY: u32 = 0x0200;

    let (root_name, path) = key.split_once('\\')?;
    let root = match root_name.to_ascii_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE" => LOCAL_MACHINE,
        "HKCU" | "HKEY_CURRENT_USER" => CURRENT_USER,
        _ => return None,
    };
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let Ok(data) = root.options().read().access(view).open(path).and_then(|k| k.get_string(value)) else {
            continue;
        };
        let data = data.trim();
        if !data.is_empty() {
            return Some(expand_path_vars(data));
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_registry_string(_key: &str, _value: &str) -> Option<String> {
    None
}

/// Expand environment variables and home directory references in paths.
/// Handles `%VAR%` on Windows and `~` on all platforms.
pub(crate) fn expand_path_vars(path: &str) -> String {
    let mut result = path.to_string();

    // Expand ~ to home directory
    if result.starts_with("~/") || result == "~" {
        if let Some(home) = dirs::home_dir() {
            result = result.replacen("~", &home.to_string_lossy(), 1);
        }
    }

    // Expand %VAR% environment variables (Windows-style)
    while let Some(start) = result.find('%') {
        if let Some(end) = result[start + 1..].find('%') {
            let var_name = &result[start + 1..start + 1 + end];
            if let Ok(val) = std::env::var(var_name) {
                result = format!("{}{}{}", &result[..start], val, &result[start + 2 + end..]);
            } else {
                break; // Unknown variable, stop expanding
            }
        } else {
            break; // No closing %, stop
        }
    }

    result
}

/// Get platform-specific paths from PlatformPaths.
fn get_platform_paths(paths: &crate::registry::PlatformPaths) -> &[String] {
    #[cfg(target_os = "windows")]
    {
        &paths.windows
    }
    #[cfg(target_os = "macos")]
    {
        &paths.macos
    }
    #[cfg(target_os = "linux")]
    {
        &paths.linux
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        &[]
    }
}

/// Detect a game path by looking up its definition in the registry.
pub fn detect_game_path_from_registry(game_id: &str, registry: &GameRegistry) -> Option<String> {
    let game_def = registry.games.iter().find(|g| g.id == game_id)?;
    detect_game_path_from_def(game_def)
}

/// Every extension listed by any of the game's content types (for messages).
pub fn valid_extensions_for_game(game_def: &GameDefinition) -> Vec<String> {
    let mut exts: Vec<String> = Vec::new();
    for e in game_def.content_types.iter().flat_map(|ct| ct.extensions.iter()) {
        let e = e.to_lowercase();
        if !exts.contains(&e) {
            exts.push(e);
        }
    }
    exts
}

/// Path components compared case-insensitively on Windows (NTFS is), exactly elsewhere.
fn path_key(p: &std::path::Path) -> Vec<String> {
    p.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| {
            let s = c.as_os_str().to_string_lossy().to_string();
            if cfg!(target_os = "windows") {
                s.trim_end_matches(['\\', '/']).to_lowercase()
            } else {
                s
            }
        })
        .collect()
}

/// True if `a` and `b` are the same folder or one is inside the other.
/// Both should already be canonical/cleaned.
pub fn paths_overlap(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (a, b) = (path_key(a), path_key(b));
    let n = a.len().min(b.len());
    n > 0 && a[..n] == b[..n]
}

pub fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    path_key(a) == path_key(b)
}

/// Folders a game path must never be: picking one made SyncCrate scan (and
/// create `Mods/`, `Saves/` in) a whole drive or the user's Documents.
pub fn protected_folders() -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    for (dir, what) in [
        (dirs::home_dir(), "your user folder"),
        (dirs::document_dir(), "your Documents folder"),
        (dirs::desktop_dir(), "your Desktop"),
        (dirs::download_dir(), "your Downloads folder"),
    ] {
        if let Some(d) = dir {
            let d = std::fs::canonicalize(&d).map(clean_path).unwrap_or(d);
            out.push((d, what));
        }
    }
    out
}

/// Why `path` (canonical) can't be a game folder, if it's a drive root or one
/// of `protected`.
pub fn protected_folder_reason(path: &std::path::Path, protected: &[(PathBuf, &'static str)]) -> Option<String> {
    if path.parent().is_none() || path_key(path).len() <= 1 {
        return Some("a drive root".to_string());
    }
    protected
        .iter()
        .find(|(p, _)| same_path(p, path))
        .map(|(_, what)| what.to_string())
}

/// Get the path for a specific content type folder.
#[allow(dead_code)]
pub fn content_type_path(base: &str, folder: &str) -> PathBuf {
    PathBuf::from(base).join(folder)
}

pub fn profiles_dir() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate").join("profiles");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn timestamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Validate that a relative path does not escape the base directory.
/// Rejects absolute paths, ".." components, and returns a canonical path
/// to prevent TOCTOU symlink attacks.
pub fn safe_join(base: &str, relative: &str) -> Result<PathBuf, String> {
    let rel = std::path::Path::new(relative);
    if rel.is_absolute() {
        return Err(format!("Absolute path rejected: {}", relative));
    }

    // Only plain path segments are allowed. On Windows `is_absolute()` is false
    // for root-relative ("\\Windows\\x") and drive-relative ("C:x") paths, yet
    // `PathBuf::join` with either replaces (part of) the base — so reject any
    // Prefix/RootDir component explicitly, not just "..".
    for component in rel.components() {
        match component {
            std::path::Component::Normal(name) if is_unsafe_windows_name(&name.to_string_lossy()) => {
                return Err(format!("Invalid file name in path: {}", relative));
            }
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(format!("Path traversal rejected: {}", relative));
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(format!("Absolute path rejected: {}", relative));
            }
        }
    }

    // ':' is never valid in a Windows file name; "file.package:stream" would
    // write an NTFS alternate data stream instead of a regular file.
    #[cfg(target_os = "windows")]
    if relative.contains(':') {
        return Err(format!("Invalid character in path: {}", relative));
    }

    // Rebuild from the normal components only (drops "." segments so the
    // ancestor walk below sees a clean path).
    let mut joined = PathBuf::from(base);
    for component in rel.components() {
        if let std::path::Component::Normal(name) = component {
            joined.push(name);
        }
    }

    let base_canonical = std::fs::canonicalize(base)
        .map_err(|e| format!("Cannot resolve base path: {}", e))?;

    // Find the deepest ancestor (or the path itself) that exists and resolve it.
    // Checking only the immediate parent let a not-yet-existing subpath below a
    // symlink/junction that points outside the base slip through unchecked
    // (e.g. "Mods/link_to_C_drive/newdir/file").
    let mut existing = joined.as_path();
    let mut remainder: Vec<&std::ffi::OsStr> = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                remainder.push(name);
                existing = parent;
            }
            _ => return Err(format!("Cannot resolve path: {}", relative)),
        }
    }

    let existing_canonical = std::fs::canonicalize(existing)
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    if !existing_canonical.starts_with(&base_canonical) {
        return Err(format!("Path escapes base directory: {}", relative));
    }

    let mut result = existing_canonical;
    for name in remainder.into_iter().rev() {
        result.push(name);
    }
    Ok(result)
}

pub fn metadata_path() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("mod_metadata.json")
}

pub fn backups_dir() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate").join("backups");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn sync_config_path() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("sync_config.json")
}

pub fn game_config_path() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("game_config.json")
}

pub fn hash_cache_path() -> PathBuf {
    let config = config_root();
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hash_cache.json")
}

/// Extensions that should never be accepted from a peer during sync.
/// Note: .jar and .dll are intentionally excluded — they are legitimate mod
/// formats for Minecraft Java and Stardew Valley (SMAPI) respectively.
/// Those are handled by the per-game `dangerous_script_extensions` warning system instead.
/// Also Explorer-triggered ones (.url/.scf/.library-ms/.search-ms can leak
/// the user's NTLM hash just by opening the folder) and other launchable or
/// mountable types; none of them is a mod format in the registry.
const DANGEROUS_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "ps1", "vbs", "scr", "lnk",
    "sys", "com", "pif", "msi", "app", "sh", "bash",
    "cpl", "inf", "reg", "ws", "wsf", "hta",
    "url", "scf", "library-ms", "search-ms", "js", "jse", "vbe", "wsh", "msc", "psm1", "psd1",
    "chm", "iso", "img", "vhd", "vhdx", "appref-ms", "settingcontent-ms", "msp", "mst", "application",
    "gadget", "jnlp", "diagcab", "appx", "msix", "appinstaller", "command",
];
/// Whole file names blocked the same way (Explorer reads them on open).
const DANGEROUS_FILE_NAMES: &[&str] = &["desktop.ini", "autorun.inf"];

/// Returns true if the file extension is on the blocklist of dangerous executables.
/// Sees through a `.disabled` suffix: `evil.exe.disabled` is one rename away
/// from running, so it's blocked like `evil.exe`.
pub fn is_dangerous_extension(path: &str) -> bool {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let name = name.strip_suffix(crate::commands::files::DISABLED_SUFFIX).unwrap_or(&name);
    if DANGEROUS_FILE_NAMES.contains(&name) {
        return true;
    }
    let ext = crate::commands::files::effective_extension(p);
    DANGEROUS_EXTENSIONS.contains(&ext.as_str())
}

/// Windows can't create (or silently alters) these names: device names like
/// `CON`/`NUL`/`COM1` (with any extension) and names ending in a dot or
/// space (`evil.exe.` is stored as `evil.exe` by some APIs and literally by
/// others, and dodges the extension blocklist either way).
fn is_unsafe_windows_name(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit())
}

/// Absolute path of a Windows system program (`%SystemRoot%\System32\<name>`,
/// or `%SystemRoot%\<name>` for explorer.exe). Spawning a bare name made
/// Windows look in the app's own (user-writable) folder first.
#[cfg(target_os = "windows")]
pub fn windows_system_exe(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    if name.eq_ignore_ascii_case("explorer.exe") {
        root.join(name)
    } else if name.eq_ignore_ascii_case("powershell.exe") {
        root.join("System32").join("WindowsPowerShell").join("v1.0").join(name)
    } else {
        root.join("System32").join(name)
    }
}

/// Migrate config from the old `simshare` directory to `synccrate`.
/// Safe to call multiple times — does nothing if synccrate dir already exists.
pub fn migrate_from_simshare() {
    let config = match dirs::config_dir() {
        Some(c) => c,
        None => return,
    };
    let new_dir = config.join("synccrate");
    let old_dir = config.join("simshare");

    if new_dir.exists() && std::fs::read_dir(&new_dir).ok().map_or(false, |mut d| d.next().is_some()) {
        migrate_profile_extensions(&new_dir.join("profiles"));
        return;
    }

    if !old_dir.exists() {
        return;
    }

    log::info!("Migrating config from simshare to synccrate");

    if let Err(e) = copy_dir_recursive(&old_dir, &new_dir) {
        log::error!("Config migration failed: {}", e);
        return;
    }

    migrate_profile_extensions(&new_dir.join("profiles"));

    log::info!("Config migration complete");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn migrate_profile_extensions(profiles_dir: &std::path::Path) {
    if !profiles_dir.exists() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("simshare-profile") {
                let new_path = path.with_extension("synccrate-profile");
                if !new_path.exists() {
                    let _ = std::fs::copy(&path, &new_path);
                }
            }
        }
    }
}

/// Validate a profile ID contains no path separators or traversal
pub fn sanitize_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("ID cannot be empty".to_string());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        return Err("Invalid ID: contains path separators or traversal".to_string());
    }
    // A whitelist, not a blacklist: on Windows "D:evil" joined onto a folder
    // replaces it with drive D's current directory.
    if id.len() > 128 || id.starts_with('.') || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("Invalid ID".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_windows_names_and_dangerous_types_are_refused() {
        let dir = std::env::temp_dir().join(format!("sc-names-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.to_str().unwrap();
        for bad in ["Mods/CON", "Mods/nul.package", "Mods/com1.txt", "Mods/LPT9", "Mods/evil.exe.", "Mods/evil.exe ", "Mods/dir./x.package"] {
            assert!(safe_join(base, bad).is_err(), "{bad} should be rejected");
        }
        for ok in ["Mods/console.package", "Mods/COM10.package", "Mods/a.b.package", "Mods/nul_hair.package"] {
            assert!(safe_join(base, ok).is_ok(), "{ok} should be allowed");
        }
        for bad in ["Mods/x.url", "Mods/x.scf", "Mods/desktop.ini", "Mods/DESKTOP.INI.disabled", "Mods/x.library-ms", "Mods/x.js", "Mods/x.iso"] {
            assert!(is_dangerous_extension(bad), "{bad} should be blocked");
        }
        assert!(!is_dangerous_extension("Mods/x.package"));
        assert!(!is_dangerous_extension("mods/sodium.jar"), "jar/dll stay allowed (real mod formats)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_are_whitelisted() {
        for ok in ["0b7c7c2e-1a2b-4c3d-9e8f-001122334455", "backup_2026-09-25", "a.b"] {
            assert!(sanitize_id(ok).is_ok(), "{ok}");
        }
        for bad in ["D:evil", "C:x", "a/b", "..", ".hidden", "a b", "x\u{0}", ""] {
            assert!(sanitize_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn test_safe_join_normal_path() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let result = safe_join(&base_str, "Mods/file.package");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("Mods"));
        assert!(path.to_string_lossy().contains("file.package"));
    }

    #[test]
    fn test_safe_join_blocks_traversal() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let result = safe_join(&base_str, "../../../etc/passwd");
        assert!(result.is_err(), "path traversal should be rejected");
    }

    #[test]
    fn test_safe_join_blocks_absolute_path() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        #[cfg(target_os = "windows")]
        let result = safe_join(&base_str, "C:\\Windows\\System32\\cmd.exe");
        #[cfg(not(target_os = "windows"))]
        let result = safe_join(&base_str, "/etc/passwd");
        assert!(result.is_err(), "absolute paths should be rejected");
    }

    #[test]
    fn test_safe_join_nested_nonexistent_dirs_stay_in_base() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let path = safe_join(&base_str, "synccrate_no_such_dir/a/b/file.package").unwrap();
        let base_canonical = std::fs::canonicalize(&base).unwrap();
        assert!(path.starts_with(&base_canonical));
        assert!(path.ends_with("synccrate_no_such_dir/a/b/file.package"));
    }

    #[test]
    fn test_safe_join_ignores_curdir_segments() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        let path = safe_join(&base_str, "./Mods/./x.package").unwrap();
        assert!(path.ends_with("Mods/x.package"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_safe_join_blocks_windows_root_and_drive_relative() {
        let base = std::env::temp_dir();
        let base_str = base.to_string_lossy();
        // Not `is_absolute()` on Windows, but join() would escape the base.
        assert!(safe_join(&base_str, "\\Windows\\System32\\evil.dll").is_err());
        assert!(safe_join(&base_str, "/Windows/evil.dll").is_err());
        assert!(safe_join(&base_str, "C:evil.package").is_err());
        // NTFS alternate data stream
        assert!(safe_join(&base_str, "Mods/a.package:stream").is_err());
    }

    #[test]
    fn paths_overlap_detects_nesting_both_ways() {
        let base = std::env::temp_dir();
        let a = base.join("Games").join("Sims");
        assert!(paths_overlap(&a, &a));
        assert!(paths_overlap(&a, &a.join("Mods")));
        assert!(paths_overlap(&a.join("Mods"), &a));
        assert!(!paths_overlap(&a, &base.join("Games").join("Sims 4")));
        assert!(!paths_overlap(&a, &base.join("Games").join("Other")));
        #[cfg(target_os = "windows")]
        {
            use std::path::Path;
            assert!(paths_overlap(Path::new(r"C:\Games\Sims"), Path::new(r"c:\games\SIMS\Mods")));
            assert!(same_path(Path::new(r"C:\Games\Sims\"), Path::new(r"c:\games\sims")));
            assert!(paths_overlap(Path::new(r"C:\"), Path::new(r"C:\Games")));
        }
    }

    #[test]
    fn protected_folder_reason_rejects_roots_and_user_folders() {
        let docs = std::env::temp_dir().join("synccrate_fake_docs");
        let protected = vec![(docs.clone(), "your Documents folder")];
        #[cfg(target_os = "windows")]
        {
            assert_eq!(protected_folder_reason(std::path::Path::new(r"C:\"), &protected).as_deref(), Some("a drive root"));
            assert_eq!(protected_folder_reason(std::path::Path::new(r"D:\"), &protected).as_deref(), Some("a drive root"));
        }
        #[cfg(not(target_os = "windows"))]
        assert_eq!(protected_folder_reason(std::path::Path::new("/"), &protected).as_deref(), Some("a drive root"));
        assert_eq!(protected_folder_reason(&docs, &protected).as_deref(), Some("your Documents folder"));
        // A game folder *inside* Documents is fine.
        assert_eq!(protected_folder_reason(&docs.join("Electronic Arts").join("The Sims 4"), &protected), None);
    }

    #[test]
    fn valid_extensions_are_deduplicated() {
        let g: GameDefinition = serde_json::from_value(serde_json::json!({
            "id": "g", "label": "G", "family": "g",
            "content_types": [
                {"id": "a", "label": "A", "folder": "A", "extensions": ["zip", "PAK"], "file_type": "Mod"},
                {"id": "b", "label": "B", "folder": "B", "extensions": ["zip"], "file_type": "Save"}
            ]
        }))
        .unwrap();
        assert_eq!(valid_extensions_for_game(&g), vec!["zip".to_string(), "pak".to_string()]);
    }

    #[test]
    fn test_sanitize_id_valid() {
        let result = sanitize_id("abc-123-def");
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_id_rejects_slashes() {
        assert!(sanitize_id("../malicious").is_err());
        assert!(sanitize_id("path/to/file").is_err());
    }

    #[test]
    fn test_sanitize_id_rejects_dots() {
        assert!(sanitize_id("..").is_err());
    }

    #[test]
    fn test_dangerous_extension_blocks_executables() {
        assert!(is_dangerous_extension("file.exe"));
        assert!(is_dangerous_extension("script.bat"));
        assert!(is_dangerous_extension("script.cmd"));
        assert!(is_dangerous_extension("file.scr"));
    }

    #[test]
    fn test_dangerous_extension_allows_mod_formats() {
        assert!(!is_dangerous_extension("mod.jar"));
        assert!(!is_dangerous_extension("mod.dll"));
        assert!(!is_dangerous_extension("cc.package"));
        assert!(!is_dangerous_extension("config.xml"));
        assert!(!is_dangerous_extension("texture.png"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_read_registry_string_native() {
        let pf = read_registry_string(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion", "ProgramFilesDir");
        assert!(pf.is_some_and(|p| std::path::Path::new(&p).exists()));
        assert!(read_registry_string(r"HKLM\SOFTWARE\SyncCrateNoSuchKey", "x").is_none());
        assert!(read_registry_string(r"BOGUS\SOFTWARE", "x").is_none());
    }

    #[test]
    fn test_parse_steam_library_vdf_modern_and_legacy() {
        let modern = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        let libs = parse_steam_library_vdf(modern);
        assert_eq!(libs.len(), 2);
        assert_eq!(libs[1], std::path::PathBuf::from(r"D:\SteamLibrary"));

        let legacy = r#"
"LibraryFolders"
{
	"TimeNextStatsReport"		"1234"
	"1"		"E:\\Games\\Steam"
}
"#;
        let libs = parse_steam_library_vdf(legacy);
        assert_eq!(libs, vec![std::path::PathBuf::from(r"E:\Games\Steam")]);
    }
}
