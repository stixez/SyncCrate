//! Mod info from the metadata files mods already ship (offline, no APIs):
//!
//! - Thunderstore / r2modman (`manifest.json` with `version_number`, `icon.png`)
//! - SMAPI for Stardew Valley (`manifest.json` with `UniqueID`)
//! - Minecraft jars: Fabric `fabric.mod.json`, Quilt `quilt.mod.json`, Forge /
//!   NeoForge `META-INF/(neoforge.)mods.toml`, old Forge `mcmod.info`
//! - Paradox games (`descriptor.mod`, `thumbnail.png` / `picture=`)
//! - Mount & Blade II: Bannerlord (`SubModule.xml`)
//!
//! Candidates come from the scanned manifest only (each file's own folder and
//! its parents, plus `.jar` files themselves), never from walking a content
//! folder raw, so exclude rules and content types still decide what's looked at.
//! Every file read here was written by a mod author, i.e. untrusted: sizes are
//! capped before reading, symlinks are skipped, strings are cleaned and
//! capped, and icon paths from a metadata file are plain file names only.
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

const MAX_META_BYTES: u64 = 256 * 1024;
const MAX_ICON_BYTES: u64 = 1024 * 1024;
/// A jar's central directory is read in full by `zip`; skip absurd archives.
const MAX_JAR_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_NAME: usize = 128;
const MAX_VERSION: usize = 64;
const MAX_DESC: usize = 1000;
const MAX_AUTHORS: usize = 10;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ModMeta {
    /// Relative path (forward slashes) of the mod's folder, or of the file
    /// itself for single-file mods (jars). Files under a folder key belong to it.
    pub key: String,
    pub is_file: bool,
    /// "thunderstore" | "smapi" | "fabric" | "quilt" | "forge" | "paradox" | "bannerlord"
    pub source: String,
    pub id: Option<String>,
    pub name: String,
    pub version: Option<String>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    pub has_icon: bool,
    #[serde(skip)]
    pub icon: IconRef,
    /// SMAPI `UpdateKeys` ("Nexus:541", "GitHub:owner/repo"), for update checks.
    #[serde(skip)]
    pub update_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum IconRef {
    #[default]
    None,
    /// Relative path (forward slashes) of an image file in the game folder.
    File(String),
    /// An entry inside the jar at `ModMeta::key`.
    JarEntry(String),
}

// ---------------------------------------------------------------------------
// String hygiene

fn clean(s: &str, max: usize) -> Option<String> {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(max).collect();
    (!s.is_empty()).then_some(s)
}

/// Mod descriptions keep their paragraphs, but lose other control characters.
fn clean_desc(s: &str) -> Option<String> {
    let s: String = s.chars().filter(|c| *c == '\n' || !c.is_control()).collect();
    let s: String = s.trim().chars().take(MAX_DESC).collect();
    (!s.is_empty()).then_some(s)
}

/// Placeholders a build tool never filled in (`${file.jarVersion}`).
fn is_placeholder(s: &str) -> bool {
    s.contains("${")
}

fn clean_version(s: &str) -> Option<String> {
    let v = clean(s, MAX_VERSION)?;
    (!is_placeholder(&v)).then_some(v)
}

fn clean_url(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let ok = (lower.starts_with("https://") || lower.starts_with("http://")) && s.len() <= 300 && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| s.to_string())
}

/// An icon path taken from a metadata file: a single plain file name with an
/// image extension, or `None`.
fn plain_image_name(name: &str) -> Option<String> {
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    let is_image = lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg");
    (is_image && !n.is_empty() && !n.contains(['/', '\\', ':']) && n != "." && n != "..").then(|| n.to_string())
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn authors_from(v: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Some(Value::String(s)) => out.extend(s.split(',').filter_map(|a| clean(a, 64))),
        Some(Value::Array(items)) => {
            for it in items {
                let name = match it {
                    Value::String(s) => Some(s.as_str()),
                    Value::Object(_) => str_of(it, "name"),
                    _ => None,
                };
                out.extend(name.and_then(|n| clean(n, 64)));
            }
        }
        Some(Value::Object(map)) => out.extend(map.keys().filter_map(|k| clean(k, 64))),
        _ => {}
    }
    out.truncate(MAX_AUTHORS);
    out
}

/// SMAPI and some Thunderstore manifests are JSON with comments, trailing
/// commas and a BOM, which `serde_json` rejects. Strip those outside strings.
pub fn relaxed_json(text: &str) -> Option<Value> {
    let text = text.trim_start_matches('\u{feff}');
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_str) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == ',' {
            // Drop a trailing comma before `}` / `]`.
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    serde_json::from_str(&out).ok()
}

// ---------------------------------------------------------------------------
// Parsers (pure; `dir_name` is the mod folder's own name)

/// `manifest.json` is used by both Thunderstore packages and SMAPI mods.
pub fn parse_manifest_json(text: &str, dir_name: &str) -> Option<ModMeta> {
    let v = relaxed_json(text)?;
    if str_of(&v, "UniqueID").is_some() || str_of(&v, "Name").is_some() {
        return Some(ModMeta {
            source: "smapi".into(),
            id: str_of(&v, "UniqueID").and_then(|s| clean(s, MAX_NAME)),
            name: str_of(&v, "Name").and_then(|s| clean(s, MAX_NAME))?,
            version: str_of(&v, "Version").and_then(clean_version),
            authors: authors_from(v.get("Author")),
            description: str_of(&v, "Description").and_then(clean_desc),
            update_keys: v
                .get("UpdateKeys")
                .and_then(Value::as_array)
                .map(|keys| keys.iter().filter_map(Value::as_str).filter_map(|k| clean(k, 128)).take(10).collect())
                .unwrap_or_default(),
            ..Default::default()
        });
    }
    let name = str_of(&v, "name")?;
    str_of(&v, "version_number")?;
    // Thunderstore installs into `Author-Name`; the manifest itself has no author.
    let author = dir_name
        .split_once('-')
        .filter(|(a, n)| !a.is_empty() && n.replace('_', " ").eq_ignore_ascii_case(&name.replace('_', " ")))
        .and_then(|(a, _)| clean(a, 64));
    Some(ModMeta {
        source: "thunderstore".into(),
        id: Some(format!("{}{}", author.as_deref().map(|a| format!("{a}-")).unwrap_or_default(), name)).and_then(|s| clean(&s, MAX_NAME)),
        name: clean(&name.replace('_', " "), MAX_NAME)?,
        version: str_of(&v, "version_number").and_then(clean_version),
        authors: author.into_iter().collect(),
        description: str_of(&v, "description").and_then(clean_desc),
        website: str_of(&v, "website_url").and_then(clean_url),
        ..Default::default()
    })
}

/// Returns the metadata and the icon entry path inside the jar, if any.
pub fn parse_fabric(text: &str) -> Option<(ModMeta, Option<String>)> {
    let v = relaxed_json(text)?;
    let id = str_of(&v, "id")?;
    let icon = match v.get("icon") {
        Some(Value::String(s)) => Some(s.clone()),
        // A size → path map: take the largest size.
        Some(Value::Object(m)) => m.iter().filter_map(|(k, p)| Some((k.parse::<u32>().ok()?, p.as_str()?))).max_by_key(|(k, _)| *k).map(|(_, p)| p.to_string()),
        _ => None,
    };
    let meta = ModMeta {
        source: "fabric".into(),
        id: clean(id, MAX_NAME),
        name: str_of(&v, "name").and_then(|s| clean(s, MAX_NAME)).or_else(|| clean(id, MAX_NAME))?,
        version: str_of(&v, "version").and_then(clean_version),
        authors: authors_from(v.get("authors")),
        description: str_of(&v, "description").and_then(clean_desc),
        website: v.get("contact").and_then(|c| str_of(c, "homepage")).and_then(clean_url),
        ..Default::default()
    };
    Some((meta, icon))
}

pub fn parse_quilt(text: &str) -> Option<(ModMeta, Option<String>)> {
    let v = relaxed_json(text)?;
    let ql = v.get("quilt_loader")?;
    let id = str_of(ql, "id")?;
    let md = ql.get("metadata").cloned().unwrap_or(Value::Null);
    let icon = match md.get("icon") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(m)) => m.iter().filter_map(|(k, p)| Some((k.parse::<u32>().ok()?, p.as_str()?))).max_by_key(|(k, _)| *k).map(|(_, p)| p.to_string()),
        _ => None,
    };
    let meta = ModMeta {
        source: "quilt".into(),
        id: clean(id, MAX_NAME),
        name: str_of(&md, "name").and_then(|s| clean(s, MAX_NAME)).or_else(|| clean(id, MAX_NAME))?,
        version: str_of(ql, "version").and_then(clean_version),
        authors: authors_from(md.get("contributors")),
        description: str_of(&md, "description").and_then(clean_desc),
        website: md.get("contact").and_then(|c| str_of(c, "homepage")).and_then(clean_url),
        ..Default::default()
    };
    Some((meta, icon))
}

/// Just enough TOML for Forge's `mods.toml`: top-level keys and the first
/// `[[mods]]` table; basic, literal and `'''`/`"""` multi-line strings.
fn toml_tables(text: &str) -> (HashMap<String, String>, HashMap<String, String>) {
    let (mut top, mut first_mod) = (HashMap::new(), HashMap::new());
    // 0 = top level, 1 = first [[mods]], 2 = anything after
    let mut section = 0;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.starts_with('[') {
            section = match (section, t) {
                (0, "[[mods]]") => 1,
                _ => 2,
            };
            continue;
        }
        let Some((k, rest)) = t.split_once('=') else { continue };
        let key = k.trim().to_string();
        let rest = rest.trim();
        let value = if let Some(q) = ["'''", "\"\"\""].into_iter().find(|q| rest.starts_with(q)) {
            let mut body = rest[3..].to_string();
            if let Some(end) = body.find(q) {
                body.truncate(end);
            } else {
                for l in lines.by_ref() {
                    if let Some(end) = l.find(q) {
                        body.push('\n');
                        body.push_str(&l[..end]);
                        break;
                    }
                    body.push('\n');
                    body.push_str(l);
                }
            }
            body
        } else if let Some(r) = rest.strip_prefix('"') {
            let mut out = String::new();
            let mut chars = r.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some(o) => out.push(o),
                        None => {}
                    },
                    '"' => break,
                    c => out.push(c),
                }
            }
            out
        } else if let Some(r) = rest.strip_prefix('\'') {
            r.split('\'').next().unwrap_or_default().to_string()
        } else {
            continue;
        };
        match section {
            0 => { top.entry(key).or_insert(value); }
            1 => { first_mod.entry(key).or_insert(value); }
            _ => {}
        }
    }
    (top, first_mod)
}

/// `impl_version` is `Implementation-Version` from the jar's MANIFEST.MF,
/// used when mods.toml says `${file.jarVersion}`.
pub fn parse_mods_toml(text: &str, impl_version: Option<&str>) -> Option<(ModMeta, Option<String>)> {
    let (top, m) = toml_tables(text);
    let id = m.get("modId")?;
    let version = m.get("version").and_then(|v| clean_version(v)).or_else(|| impl_version.and_then(clean_version));
    let icon = m.get("logoFile").or_else(|| top.get("logoFile")).cloned();
    let meta = ModMeta {
        source: "forge".into(),
        id: clean(id, MAX_NAME),
        name: m.get("displayName").and_then(|s| clean(s, MAX_NAME)).or_else(|| clean(id, MAX_NAME))?,
        version,
        authors: m.get("authors").or_else(|| top.get("authors")).map(|a| authors_from(Some(&Value::String(a.clone())))).unwrap_or_default(),
        description: m.get("description").and_then(|d| clean_desc(d)),
        website: m.get("displayURL").or_else(|| top.get("displayURL")).and_then(|u| clean_url(u)),
        ..Default::default()
    };
    Some((meta, icon))
}

/// Pre-1.13 Forge: a JSON array (or `{"modList": [...]}`) of mods; the first wins.
pub fn parse_mcmod_info(text: &str) -> Option<(ModMeta, Option<String>)> {
    let v = relaxed_json(text)?;
    let first = match &v {
        Value::Array(a) => a.first()?.clone(),
        Value::Object(_) => v.get("modList")?.as_array()?.first()?.clone(),
        _ => return None,
    };
    let id = str_of(&first, "modid")?;
    let meta = ModMeta {
        source: "forge".into(),
        id: clean(id, MAX_NAME),
        name: str_of(&first, "name").and_then(|s| clean(s, MAX_NAME)).or_else(|| clean(id, MAX_NAME))?,
        version: str_of(&first, "version").and_then(clean_version),
        authors: authors_from(first.get("authorList").or_else(|| first.get("authors"))),
        description: str_of(&first, "description").and_then(clean_desc),
        website: str_of(&first, "url").and_then(clean_url),
        ..Default::default()
    };
    Some((meta, str_of(&first, "logoFile").map(str::to_string)))
}

/// Paradox `descriptor.mod`: `key="value"` lines (plus `tags={...}` blocks,
/// ignored). Returns the icon file name (`picture=`, else `thumbnail.png`).
pub fn parse_paradox_descriptor(text: &str) -> Option<(ModMeta, Option<String>)> {
    let mut kv: HashMap<String, String> = HashMap::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        let v = v.strip_prefix('"').map(|s| s.split('"').next().unwrap_or_default()).unwrap_or(v);
        kv.entry(k.trim().to_string()).or_insert_with(|| v.to_string());
    }
    let meta = ModMeta {
        source: "paradox".into(),
        id: kv.get("remote_file_id").filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).cloned(),
        name: kv.get("name").and_then(|s| clean(s, MAX_NAME))?,
        version: kv.get("version").and_then(|s| clean_version(s)),
        website: kv
            .get("remote_file_id")
            .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
            .map(|id| format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")),
        ..Default::default()
    };
    let icon = kv.get("picture").and_then(|p| plain_image_name(p));
    Some((meta, icon))
}

fn xml_attr_of(text: &str, tag: &str) -> Option<String> {
    let start = text.find(&format!("<{tag} "))?;
    let rest = &text[start..];
    let rest = &rest[..rest.find('>')?];
    let v = rest.split_once("value=")?.1.trim_start();
    let quote = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let v = &v[1..];
    let v = &v[..v.find(quote)?];
    Some(v.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"))
}

/// Bannerlord `SubModule.xml`: the module's own `<Name value=".."/>` etc.
/// come before the `<SubModules>` list, so the first match is the module.
pub fn parse_bannerlord(text: &str) -> Option<ModMeta> {
    let head = text.split("<SubModules").next().unwrap_or(text);
    Some(ModMeta {
        source: "bannerlord".into(),
        id: xml_attr_of(head, "Id").and_then(|s| clean(&s, MAX_NAME)),
        name: xml_attr_of(head, "Name").and_then(|s| clean(&s, MAX_NAME))?,
        version: xml_attr_of(head, "Version").and_then(|s| clean_version(&s)),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Extraction

/// A regular file (not a symlink) no bigger than `max`.
fn small_regular_file(path: &Path, max: u64) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file() && m.len() <= max)
}

fn read_small(path: &Path) -> Option<String> {
    if !small_regular_file(path, MAX_META_BYTES) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn zip_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str, max: u64) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > max {
        return None;
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(max).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn read_jar(path: &Path) -> Option<(ModMeta, Option<String>)> {
    if !small_regular_file(path, MAX_JAR_BYTES) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let text = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| zip_entry(zip, name, MAX_META_BYTES).map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(t) = text(&mut zip, "fabric.mod.json") {
        if let Some(r) = parse_fabric(&t) {
            return Some(r);
        }
    }
    if let Some(t) = text(&mut zip, "quilt.mod.json") {
        if let Some(r) = parse_quilt(&t) {
            return Some(r);
        }
    }
    for toml_name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(t) = text(&mut zip, toml_name) {
            let impl_version = text(&mut zip, "META-INF/MANIFEST.MF").and_then(|mf| {
                mf.lines().find_map(|l| l.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string()))
            });
            if let Some(r) = parse_mods_toml(&t, impl_version.as_deref()) {
                return Some(r);
            }
        }
    }
    text(&mut zip, "mcmod.info").and_then(|t| parse_mcmod_info(&t))
}

fn is_jar(rel: &str) -> bool {
    let l = rel.to_ascii_lowercase();
    l.ends_with(".jar") || l.ends_with(".jar.disabled")
}

fn dir_name(rel_dir: &str) -> &str {
    rel_dir.rsplit('/').next().unwrap_or(rel_dir)
}

fn probe_dir(base: &str, rel_dir: &str) -> Option<ModMeta> {
    let dir = crate::utils::safe_join(base, rel_dir).ok()?;
    let with = |mut meta: ModMeta, icon: Option<String>| {
        meta.key = rel_dir.to_string();
        if let Some(icon) = icon.filter(|i| small_regular_file(&dir.join(i), MAX_ICON_BYTES)) {
            meta.icon = IconRef::File(format!("{rel_dir}/{icon}"));
            meta.has_icon = true;
        }
        meta
    };
    if let Some(t) = read_small(&dir.join("manifest.json")) {
        if let Some(meta) = parse_manifest_json(&t, dir_name(rel_dir)) {
            let icon = (meta.source == "thunderstore").then(|| "icon.png".to_string());
            return Some(with(meta, icon));
        }
    }
    if let Some(t) = read_small(&dir.join("descriptor.mod")) {
        if let Some((meta, icon)) = parse_paradox_descriptor(&t) {
            return Some(with(meta, icon.or_else(|| Some("thumbnail.png".into()))));
        }
    }
    if let Some(t) = read_small(&dir.join("SubModule.xml")) {
        if let Some(meta) = parse_bannerlord(&t) {
            return Some(with(meta, None));
        }
    }
    None
}

/// Metadata for everything in `files` (relative paths from the scanned
/// manifest), rooted at `base`.
pub fn extract(base: &str, files: &[String]) -> Vec<ModMeta> {
    let mut dirs = BTreeSet::new();
    let mut jars = Vec::new();
    for f in files {
        if is_jar(f) {
            jars.push(f.clone());
        }
        let mut d = f.as_str();
        while let Some((parent, _)) = d.rsplit_once('/') {
            if !dirs.insert(parent.to_string()) {
                break; // this parent and everything above it is already queued
            }
            d = parent;
        }
    }
    let mut out: Vec<ModMeta> = dirs.iter().filter_map(|d| probe_dir(base, d)).collect();
    for rel in jars {
        let Ok(path) = crate::utils::safe_join(base, &rel) else { continue };
        if let Some((mut meta, icon)) = read_jar(&path) {
            meta.key = rel.clone();
            meta.is_file = true;
            if let Some(icon) = icon.filter(|i| !i.is_empty() && i.len() <= 256) {
                meta.icon = IconRef::JarEntry(icon.trim_start_matches('/').to_string());
                meta.has_icon = true;
            }
            out.push(meta);
        }
    }
    out
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// The icon as a `data:` URL, only if it really is a PNG or JPEG.
pub fn icon_data_url(base: &str, key: &str, icon: &IconRef) -> Option<String> {
    use base64::Engine as _;
    let bytes = match icon {
        IconRef::None => return None,
        IconRef::File(rel) => {
            let path = crate::utils::safe_join(base, rel).ok()?;
            if !small_regular_file(&path, MAX_ICON_BYTES) {
                return None;
            }
            std::fs::read(path).ok()?
        }
        IconRef::JarEntry(entry) => {
            let path = crate::utils::safe_join(base, key).ok()?;
            let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
            zip_entry(&mut zip, entry, MAX_ICON_BYTES)?
        }
    };
    let mime = image_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    #[test]
    fn thunderstore_manifest() {
        let json = r#"{"name":"Valheim_Plus","version_number":"0.9.9","website_url":"https://github.com/valheimPlus","description":"A mod","dependencies":[]}"#;
        let m = parse_manifest_json(json, "valheimPlus-Valheim_Plus").unwrap();
        assert_eq!((m.source.as_str(), m.name.as_str()), ("thunderstore", "Valheim Plus"));
        assert_eq!(m.version.as_deref(), Some("0.9.9"));
        assert_eq!(m.authors, vec!["valheimPlus".to_string()]);
        assert_eq!(m.website.as_deref(), Some("https://github.com/valheimPlus"));
        // A folder name that doesn't match the package gives no guessed author.
        assert!(parse_manifest_json(json, "SomethingElse").unwrap().authors.is_empty());
        assert!(parse_manifest_json(r#"{"name":"x"}"#, "d").is_none(), "not a package without version_number");
    }

    #[test]
    fn smapi_manifest_with_comments_bom_and_trailing_commas() {
        let json = "\u{feff}{\n  // comment\n  \"Name\": \"Lookup Anything\",\n  \"Author\": \"Pathoschild\",\n  \"Version\": \"1.40.0\", /* block */\n  \"Description\": \"See // not a comment\",\n  \"UniqueID\": \"Pathoschild.LookupAnything\",\n  \"UpdateKeys\": [\"Nexus:518\",],\n}";
        let m = parse_manifest_json(json, "LookupAnything").unwrap();
        assert_eq!(m.source, "smapi");
        assert_eq!(m.name, "Lookup Anything");
        assert_eq!(m.id.as_deref(), Some("Pathoschild.LookupAnything"));
        assert_eq!(m.authors, vec!["Pathoschild".to_string()]);
        assert_eq!(m.description.as_deref(), Some("See // not a comment"));
        assert_eq!(m.update_keys, vec!["Nexus:518".to_string()]);
    }

    #[test]
    fn fabric_and_quilt() {
        let fabric = r#"{"schemaVersion":1,"id":"sodium","version":"0.5.8","name":"Sodium","authors":["JellySquid",{"name":"IMS"}],"contact":{"homepage":"https://modrinth.com/mod/sodium"},"icon":{"16":"a.png","128":"assets/sodium/icon.png"}}"#;
        let (m, icon) = parse_fabric(fabric).unwrap();
        assert_eq!(m.name, "Sodium");
        assert_eq!(m.authors, vec!["JellySquid".to_string(), "IMS".to_string()]);
        assert_eq!(icon.as_deref(), Some("assets/sodium/icon.png"));
        let quilt = r#"{"quilt_loader":{"id":"qsl","version":"7.0","metadata":{"name":"QSL","contributors":{"Quilt":"Owner"},"icon":"q.png"}}}"#;
        let (m, icon) = parse_quilt(quilt).unwrap();
        assert_eq!((m.name.as_str(), m.version.as_deref()), ("QSL", Some("7.0")));
        assert_eq!(m.authors, vec!["Quilt".to_string()]);
        assert_eq!(icon.as_deref(), Some("q.png"));
    }

    #[test]
    fn forge_mods_toml_with_placeholder_version_and_multiline_description() {
        let toml = "modLoader=\"javafml\"\nlogoFile=\"jei.png\"\n[[mods]]\nmodId=\"jei\"\nversion=\"${file.jarVersion}\"\ndisplayName=\"Just Enough Items\"\nauthors=\"mezz, others\"\ndescription='''\nShows items\nand recipes\n'''\n[[dependencies.jei]]\nmodId=\"forge\"\n";
        let (m, icon) = parse_mods_toml(toml, Some("15.3.0")).unwrap();
        assert_eq!(m.id.as_deref(), Some("jei"));
        assert_eq!(m.name, "Just Enough Items");
        assert_eq!(m.version.as_deref(), Some("15.3.0"), "placeholder falls back to the jar manifest");
        assert_eq!(m.authors, vec!["mezz".to_string(), "others".to_string()]);
        assert_eq!(m.description.as_deref(), Some("Shows items\nand recipes"));
        assert_eq!(icon.as_deref(), Some("jei.png"));
        let (m, _) = parse_mods_toml(toml, None).unwrap();
        assert_eq!(m.version, None, "an unfilled placeholder is never shown");
    }

    #[test]
    fn mcmod_info_both_shapes() {
        let (m, _) = parse_mcmod_info(r#"[{"modid":"ic2","name":"IndustrialCraft 2","version":"2.2","authorList":["Player"]}]"#).unwrap();
        assert_eq!(m.name, "IndustrialCraft 2");
        let (m, _) = parse_mcmod_info(r#"{"modListVersion":2,"modList":[{"modid":"x"}]}"#).unwrap();
        assert_eq!(m.name, "x");
    }

    #[test]
    fn paradox_descriptor() {
        let text = "version=\"1.2\"\ntags={\n\t\"Gameplay\"\n}\nname=\"Better UI\"\npicture=\"thumb.png\"\nsupported_version=\"1.10.*\"\nremote_file_id=\"123456\"\n";
        let (m, icon) = parse_paradox_descriptor(text).unwrap();
        assert_eq!(m.name, "Better UI");
        assert_eq!(m.version.as_deref(), Some("1.2"));
        assert_eq!(m.website.as_deref(), Some("https://steamcommunity.com/sharedfiles/filedetails/?id=123456"));
        assert_eq!(icon.as_deref(), Some("thumb.png"));
        let (_, icon) = parse_paradox_descriptor("name=\"x\"\npicture=\"../../Windows/win.ini\"").unwrap();
        assert_eq!(icon, None, "a picture path must be a plain image file name");
        let (m, _) = parse_paradox_descriptor("name=\"x\"\nremote_file_id=\"12; rm\"").unwrap();
        assert_eq!(m.website, None);
    }

    #[test]
    fn bannerlord_submodule() {
        let xml = r#"<Module><Name value="Bannerlord &amp; Friends"/><Id value="BF"/><Version value="v1.0.3"/><SubModules><SubModule><Name value="Inner"/></SubModule></SubModules></Module>"#;
        let m = parse_bannerlord(xml).unwrap();
        assert_eq!(m.name, "Bannerlord & Friends");
        assert_eq!(m.id.as_deref(), Some("BF"));
        assert_eq!(m.version.as_deref(), Some("v1.0.3"));
    }

    #[test]
    fn hostile_strings_are_cleaned_and_capped() {
        let json = format!(r#"{{"Name":"{}\u0007\u202e","UniqueID":"a","Version":"1","Author":"{}","Description":"{}"}}"#, "N".repeat(500), "a,".repeat(50), "d".repeat(5000));
        let m = parse_manifest_json(&json, "x").unwrap();
        assert!(m.name.chars().count() <= MAX_NAME && !m.name.contains('\u{7}'));
        assert_eq!(m.authors.len(), MAX_AUTHORS);
        assert_eq!(m.description.unwrap().chars().count(), MAX_DESC);
        let (m, _) = parse_fabric(r#"{"id":"x","contact":{"homepage":"javascript:alert(1)"}}"#).unwrap();
        assert_eq!(m.website, None, "only http(s) links");
        assert!(relaxed_json("{not json").is_none());
    }

    fn write(base: &Path, rel: &str, bytes: &[u8]) {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn jar(base: &Path, rel: &str, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut z = zip::ZipWriter::new(std::fs::File::create(p).unwrap());
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn extract_finds_folder_and_jar_mods_from_manifest_paths_only() {
        let base = crate::testutil::temp_dir("mod-meta");
        write(&base, "BepInEx/plugins/Auth-Cool_Mod/manifest.json", br#"{"name":"Cool_Mod","version_number":"1.0.0"}"#);
        write(&base, "BepInEx/plugins/Auth-Cool_Mod/icon.png", PNG);
        write(&base, "BepInEx/plugins/Auth-Cool_Mod/sub/CoolMod.dll", b"MZ");
        // A mod folder no scanned file lives in is never looked at.
        write(&base, "BepInEx/plugins/Unscanned/manifest.json", br#"{"name":"Hidden","version_number":"1"}"#);
        jar(&base, "mods/sodium.jar", &[("fabric.mod.json", br#"{"id":"sodium","name":"Sodium","icon":"icon.png"}"#), ("icon.png", PNG)]);
        jar(&base, "mods/broken.jar", &[("readme.txt", b"hi")]);
        write(&base, "mods/notajar.jar", b"garbage");

        let files = vec![
            "BepInEx/plugins/Auth-Cool_Mod/sub/CoolMod.dll".to_string(),
            "mods/sodium.jar".to_string(),
            "mods/broken.jar".to_string(),
            "mods/notajar.jar".to_string(),
        ];
        let b = base.to_str().unwrap();
        let metas = extract(b, &files);
        assert_eq!(metas.len(), 2, "{metas:?}");
        let cool = metas.iter().find(|m| m.key == "BepInEx/plugins/Auth-Cool_Mod").unwrap();
        assert_eq!(cool.name, "Cool Mod");
        assert!(cool.has_icon && !cool.is_file);
        let sodium = metas.iter().find(|m| m.key == "mods/sodium.jar").unwrap();
        assert!(sodium.is_file && sodium.has_icon);

        let url = icon_data_url(b, &cool.key, &cool.icon).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(icon_data_url(b, &sodium.key, &sodium.icon).unwrap().starts_with("data:image/png"));
        // Not an image: no data URL, even with a .png name.
        write(&base, "BepInEx/plugins/Auth-Cool_Mod/icon.png", b"<script>");
        assert!(icon_data_url(b, &cool.key, &cool.icon).is_none());
        // Traversal in an icon reference is refused by safe_join.
        assert!(icon_data_url(b, "x", &IconRef::File("../../etc/passwd".into())).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }
}
