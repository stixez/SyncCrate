//! CurseForge names, pictures and updates for Sims 4, Minecraft (Java) and
//! World of Warcraft addons, plus missing dependencies and Minecraft jars
//! made for another game version or loader.
//!
//! CurseForge identifies a file by a "fingerprint" (MurmurHash2 of its bytes
//! with whitespace removed), so a mod is found without sending its name or
//! contents. Their API needs a key that must never ship in an open source
//! app, so the app asks SyncCrate's own proxy (`LOOKUP_URL`, a Cloudflare
//! Pages Function in the website repo), which holds the key and forwards the
//! fingerprints. Only runs when the user clicks Check for updates.
//!
//! CurseForge's terms forbid caching or storing their data: results live in
//! memory for the Content page that asked (the frontend drops them when the
//! page closes) and logos are fetched on demand, never written to disk.
//! Every response is untrusted: sizes are capped, strings cleaned, URLs https.
use crate::mod_meta::{self, IconRef, ModMeta};
use crate::mod_updates::{ModUpdate, ModWarning};
use crate::registry::CurseForgeSupport;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

pub const LOOKUP_URL: &str = "https://synccrate.app/api/curseforge/fingerprints";
/// The proxy refuses more than this per request.
pub const MAX_PER_REQUEST: usize = 1000;
/// A Sims 4 Mods folder can hold 20k+ packages; past this only the first
/// ones (by path) are looked up, and the report says so.
pub const MAX_FILES: usize = 10_000;
/// Nothing on CurseForge for these games is this big; reading it would only
/// keep the check spinning.
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
/// Files up to this size are read whole (one pass); bigger ones are streamed
/// twice (count, then hash), so a huge file never sits in memory.
const ONE_SHOT_BYTES: u64 = 16 * 1024 * 1024;
/// Fingerprinting reads every file; on a slow disk a big CC folder could take
/// many minutes. Past this it stops and looks up what it has.
const FINGERPRINT_BUDGET: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOGO_BYTES: usize = 1024 * 1024;
const MAX_MATCHES: usize = 20_000;

// ---------------------------------------------------------------------------
// Fingerprint

const M: u32 = 0x5bd1e995;
const SEED: u32 = 1;

/// CurseForge leaves these out before hashing, so line-ending and
/// indentation changes don't make a "different" file.
fn is_skipped(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0D | 0x20)
}

/// MurmurHash2 (32-bit) fed incrementally. The seed is mixed with the total
/// length up front, which is why the caller must know the filtered length
/// before the first byte.
struct Murmur2 {
    h: u32,
    tail: [u8; 4],
    tail_len: usize,
}

impl Murmur2 {
    fn new(len: u32) -> Self {
        Self { h: SEED ^ len, tail: [0; 4], tail_len: 0 }
    }

    fn update(&mut self, chunk: &[u8]) {
        for &b in chunk {
            if is_skipped(b) {
                continue;
            }
            self.tail[self.tail_len] = b;
            self.tail_len += 1;
            if self.tail_len == 4 {
                let mut k = u32::from_le_bytes(self.tail);
                k = k.wrapping_mul(M);
                k ^= k >> 24;
                k = k.wrapping_mul(M);
                self.h = self.h.wrapping_mul(M) ^ k;
                self.tail_len = 0;
            }
        }
    }

    fn finish(self) -> u32 {
        let (mut h, t) = (self.h, self.tail);
        if self.tail_len >= 3 {
            h ^= (t[2] as u32) << 16;
        }
        if self.tail_len >= 2 {
            h ^= (t[1] as u32) << 8;
        }
        if self.tail_len >= 1 {
            h ^= t[0] as u32;
            h = h.wrapping_mul(M);
        }
        h ^= h >> 13;
        h = h.wrapping_mul(M);
        h ^ (h >> 15)
    }
}

fn kept_len(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|b| !is_skipped(**b)).count() as u64
}

/// CurseForge's fingerprint of `bytes`.
pub fn fingerprint(bytes: &[u8]) -> u32 {
    let mut h = Murmur2::new(kept_len(bytes) as u32);
    h.update(bytes);
    h.finish()
}

/// The same as `fingerprint` over a stream, reading it twice in `buf_size`
/// chunks: once to count the kept bytes, once to hash them.
pub fn fingerprint_stream<R: Read + Seek>(r: &mut R, buf_size: usize) -> std::io::Result<u32> {
    let mut buf = vec![0u8; buf_size.max(1)];
    let start = r.stream_position()?;
    let mut len = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        len += kept_len(&buf[..n]);
    }
    if len > u32::MAX as u64 {
        return Err(std::io::Error::other("file too large"));
    }
    r.seek(SeekFrom::Start(start))?;
    let mut h = Murmur2::new(len as u32);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finish())
}

/// A regular file's fingerprint; None for symlinks, huge or unreadable files.
fn fingerprint_file(path: &Path) -> Option<u32> {
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.file_type().is_file() || md.len() > MAX_FILE_BYTES {
        return None;
    }
    if md.len() <= ONE_SHOT_BYTES {
        return Some(fingerprint(&std::fs::read(path).ok()?));
    }
    let mut f = std::fs::File::open(path).ok()?;
    fingerprint_stream(&mut f, 1 << 20).ok()
}

/// Fingerprints of `files` (relative to `base`) in parallel. The bool says
/// whether every file was reached before the time budget ran out.
fn fingerprint_files(base: &str, files: &[String]) -> (Vec<(String, u32)>, bool) {
    use rayon::prelude::*;
    let started = std::time::Instant::now();
    let out_of_time = std::sync::atomic::AtomicBool::new(false);
    let prints = files
        .par_iter()
        .filter_map(|rel| {
            if started.elapsed() >= FINGERPRINT_BUDGET {
                out_of_time.store(true, std::sync::atomic::Ordering::Relaxed);
                return None;
            }
            let path = crate::utils::safe_join(base, rel).ok()?;
            Some((rel.clone(), fingerprint_file(&path)?))
        })
        .collect();
    (prints, !out_of_time.into_inner())
}

/// Which scanned files CurseForge can know, for a game the registry says it
/// has (`cf`): Sims 4 `.package` / `.ts4script` and Minecraft `.jar` files
/// in the mods content folder (`mods_folder`, as manifest paths spell it),
/// disabled ones included. Sorted, so a capped check always takes the same files.
pub fn select_files(cf: &CurseForgeSupport, mods_folder: &str, files: &[String]) -> Vec<String> {
    let exts: &[&str] = match cf.game.as_str() {
        "sims4" => &["package", "ts4script"],
        "minecraft_java" => &["jar"],
        // WoW addons are identified per folder, from the files their .toc loads.
        WOW_GAME if mods_folder.eq_ignore_ascii_case(WOW_ADDONS) => &["toc", "xml", "lua"],
        _ => return Vec::new(),
    };
    let prefix = format!("{}/", mods_folder.trim_end_matches('/').to_ascii_lowercase());
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| {
            let lower = f.to_ascii_lowercase();
            let name = lower.strip_suffix(".disabled").unwrap_or(&lower);
            lower.starts_with(&prefix) && exts.iter().any(|e| name.rsplit_once('.').is_some_and(|(_, x)| x == *e))
        })
        .cloned()
        .collect();
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// WoW addon folders
//
// CurseForge knows a WoW addon by each of its folders: the fingerprint of
// the files the folder's .toc loads, combined. The method below was checked
// against the live API (DBM, WeakAuras, BigWigs and Questie: every folder
// matched); it follows the description of what open source addon managers
// do, with two corrections that matching needed: any `Name-x.toc` /
// `Name_x.toc` suffix counts (Questie ships `_Forever` and `_Camelot`), and
// `Bindings.xml` is hashed without being followed.

/// Where every WoW client keeps addons (registry content folder).
pub const WOW_ADDONS: &str = "Interface/AddOns";
/// .toc and .xml files are read to follow what they load. Not small: QuestieDB
/// bakes its database into .toc metadata, up to 57 MB per client.
const MAX_LOADER_BYTES: u64 = 128 * 1024 * 1024;
/// Stops a folder whose files include each other in circles or explode.
const MAX_ADDON_FILES: usize = 5000;

/// The proxy's key for WoW, whose clients the registry tells apart by
/// flavour (`CurseForgeSupport::flavor`).
const WOW_GAME: &str = "wow";

/// Whether this game's mods are WoW addon folders rather than single files.
fn by_folder(cf: &CurseForgeSupport) -> bool {
    cf.game == WOW_GAME
}

/// `<Name>.toc` or `<Name>` + `-`/`_` + any suffix + `.toc`, compared
/// without case, directly in the folder.
fn is_own_toc(folder: &str, file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".toc") else { return false };
    let folder = folder.to_ascii_lowercase();
    !file.contains('/') && (stem == folder || stem.strip_prefix(folder.as_str()).is_some_and(|rest| rest.starts_with(['-', '_'])))
}

/// XML without its `<!-- -->` comments (they can span lines).
fn strip_xml_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i + 4..].find("-->") {
            Some(j) => rest = &rest[i + 4 + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Every `<Script file="x"/>` and `<Include file="x"/>` in `xml` (comments
/// already stripped), in document order. The whole text is scanned rather
/// than line by line: minified addons put several on one line, and missing
/// one gives a folder fingerprint CurseForge never matches.
fn xml_includes(xml: &str) -> Vec<&str> {
    let lower = xml.to_ascii_lowercase();
    let mut out = Vec::new();
    for (start, _) in lower.match_indices('<') {
        let after = &lower[start + 1..];
        let Some(tag_len) = ["include", "script"].iter().find(|t| after.starts_with(**t)).map(|t| t.len()) else { continue };
        let rest = &after[tag_len..];
        let attrs = rest.trim_start();
        if attrs.len() == rest.len() || !attrs.starts_with("file=") {
            continue;
        }
        let value = &attrs[5..];
        if !value.starts_with(['"', '\'']) {
            continue;
        }
        let Some(end) = value[1..].find(['"', '\'']) else { continue };
        if !value[1 + end + 1..].trim_start().starts_with("/>") {
            continue;
        }
        // Same byte offsets in the original: lowercasing ASCII keeps lengths.
        let from = xml.len() - value.len() + 1;
        out.push(&xml[from..from + end]);
    }
    out
}

/// What one .toc or .xml file loads, as written (relative to its folder).
fn includes(text: &str, xml: bool) -> Vec<String> {
    if xml {
        return xml_includes(&strip_xml_comments(text)).into_iter().map(str::to_string).collect();
    }
    // A .toc line loads a file when, without its `#` comment, it names a .lua or .xml.
    text.lines()
        .filter_map(|l| {
            let l = l.split('#').next().unwrap_or("").trim();
            let lower = l.to_ascii_lowercase();
            (lower.ends_with(".lua") || lower.ends_with(".xml")).then(|| l.to_string())
        })
        .collect()
}

/// The files of one addon folder that CurseForge hashes: the folder's own
/// .toc files, `Bindings.xml`, and everything the .toc and .xml files load,
/// recursively. `files` are paths inside the folder (forward slashes);
/// lookups ignore case like the game does. `read` reads one of them.
pub fn addon_load_list(folder: &str, files: &[String], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<String> {
    let mut todo: Vec<String> = files.iter().filter(|f| is_own_toc(folder, f)).cloned().collect();
    if todo.is_empty() {
        // Not an addon the game loads (a library's leftovers, a backup).
        return Vec::new();
    }
    let by_lower: HashMap<String, &String> = files.iter().map(|f| (f.to_ascii_lowercase(), f)).collect();
    let mut listed: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(b) = by_lower.get("bindings.xml") {
        seen.insert(b.to_ascii_lowercase());
        listed.push((*b).clone());
    }
    todo.sort();
    todo.reverse();
    while let Some(want) = todo.pop() {
        if listed.len() >= MAX_ADDON_FILES {
            break;
        }
        let Some(file) = by_lower.get(&want.to_ascii_lowercase()).copied() else { continue };
        if !seen.insert(file.to_ascii_lowercase()) {
            continue;
        }
        listed.push(file.clone());
        let lower = file.to_ascii_lowercase();
        let xml = lower.ends_with(".xml");
        if !xml && !lower.ends_with(".toc") {
            continue;
        }
        let Some(bytes) = read(file) else { continue };
        let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        // Depth first, in file order, like the game reads them.
        let mut found: Vec<String> = includes(&String::from_utf8_lossy(&bytes), xml)
            .into_iter()
            .map(|inc| inc.replace('\\', "/"))
            .filter(|inc| !inc.split('/').any(|p| p == "..") && !inc.starts_with('/') && !inc.contains(':'))
            .map(|inc| if dir.is_empty() { inc } else { format!("{dir}/{inc}") })
            .collect();
        found.reverse();
        todo.extend(found);
    }
    listed
}

/// A folder's fingerprint from its files' fingerprints: sorted, written as
/// decimal numbers with nothing between them, and hashed like a file.
pub fn folder_fingerprint(file_prints: &[u32]) -> u32 {
    let mut sorted = file_prints.to_vec();
    sorted.sort_unstable();
    let joined: String = sorted.iter().map(u32::to_string).collect();
    fingerprint(joined.as_bytes())
}

/// Each addon folder under `Interface/AddOns` with its files (paths inside
/// the folder), from the manifest paths `select_files` picked.
fn addon_folders(files: &[String]) -> Vec<(String, Vec<String>)> {
    let prefix_len = WOW_ADDONS.len() + 1;
    let mut by_folder: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for f in files {
        let Some(inside) = f.get(prefix_len..) else { continue };
        let Some((folder, rest)) = inside.split_once('/') else { continue };
        by_folder.entry(format!("{}{folder}", &f[..prefix_len])).or_default().push(rest.to_string());
    }
    by_folder.into_iter().collect()
}

/// Fingerprints of the addon folders (keyed like their `ModMeta`, e.g.
/// `Interface/AddOns/DBM-Core`), within the time budget. Folders with no
/// .toc of their own (leftovers, libraries' subfolders) have none.
fn fingerprint_addon_folders(base: &str, folders: &[(String, Vec<String>)]) -> (Vec<(String, u32)>, bool) {
    use rayon::prelude::*;
    let started = std::time::Instant::now();
    let out_of_time = std::sync::atomic::AtomicBool::new(false);
    let prints = folders
        .par_iter()
        .filter_map(|(key, files)| {
            if started.elapsed() >= FINGERPRINT_BUDGET {
                out_of_time.store(true, std::sync::atomic::Ordering::Relaxed);
                return None;
            }
            let folder = key.rsplit('/').next().unwrap_or(key);
            let dir = crate::utils::safe_join(base, key).ok()?;
            // The .toc and .xml files are read whole to follow what they
            // load; their fingerprints come from those bytes, so a 57 MB
            // QuestieDB .toc isn't read again (twice when streamed).
            let loader_prints = std::cell::RefCell::new(HashMap::new());
            let read = |rel: &str| -> Option<Vec<u8>> {
                let p = crate::utils::safe_join(dir.to_str()?, rel).ok()?;
                let md = std::fs::symlink_metadata(&p).ok()?;
                let bytes = (md.file_type().is_file() && md.len() <= MAX_LOADER_BYTES).then(|| std::fs::read(&p).ok()).flatten()?;
                loader_prints.borrow_mut().insert(rel.to_string(), fingerprint(&bytes));
                Some(bytes)
            };
            let list = addon_load_list(folder, files, &read);
            if list.is_empty() {
                return None;
            }
            let loader_prints = loader_prints.into_inner();
            let mut fps = Vec::with_capacity(list.len());
            for rel in &list {
                if let Some(fp) = loader_prints.get(rel) {
                    fps.push(*fp);
                    continue;
                }
                // A file the .toc loads that can't be read would give a
                // fingerprint CurseForge never matches; better to skip.
                fps.push(fingerprint_file(&crate::utils::safe_join(dir.to_str()?, rel).ok()?)?);
            }
            Some((key.clone(), folder_fingerprint(&fps)))
        })
        .collect();
    (prints, !out_of_time.into_inner())
}

// ---------------------------------------------------------------------------
// Response

#[derive(Debug, Clone, PartialEq)]
pub struct CfFile {
    pub id: u64,
    pub display_name: String,
    pub file_date: String,
    /// The file's page on curseforge.com, which shows its changelog.
    pub url: Option<String>,
}

/// A mod the installed file needs (CurseForge's "required dependency").
#[derive(Debug, Clone, PartialEq)]
pub struct CfDep {
    pub mod_id: u64,
    pub name: String,
    pub slug: String,
    pub website: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfMatch {
    pub fingerprints: Vec<u32>,
    pub mod_id: u64,
    pub name: String,
    pub authors: Vec<String>,
    pub summary: Option<String>,
    pub website: Option<String>,
    pub logo: Option<String>,
    pub file: Option<CfFile>,
    pub latest: Option<CfFile>,
    /// The installed file's required dependencies.
    pub requires: Vec<CfDep>,
    /// The installed file's game versions and loaders ("1.20.1", "Fabric").
    pub game_versions: Vec<String>,
}

const MAX_DEPS: usize = 50;
const MAX_GAME_VERSIONS: usize = 64;

/// A link on curseforge.com (https). The UI opens these in the browser, so
/// nothing else in those fields is trusted.
pub fn curseforge_page(s: &str) -> Option<String> {
    let url = mod_meta::clean_url(s)?;
    let parsed = reqwest::Url::parse(&url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    (parsed.scheme() == "https" && (host == "curseforge.com" || host == "www.curseforge.com")).then_some(url)
}

/// Logos come from CurseForge's CDN; anything else in that field is ignored
/// rather than fetched.
pub fn logo_url(s: &str) -> Option<String> {
    let url = mod_meta::clean_url(s)?;
    let parsed = reqwest::Url::parse(&url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let ours = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    (ours("forgecdn.net") || ours("curseforge.com")).then_some(url)
}

fn parse_file(v: Option<&Value>) -> Option<CfFile> {
    let v = v?;
    let id = v.get("id")?.as_u64()?;
    let name = v.get("displayName").and_then(Value::as_str).or_else(|| v.get("fileName").and_then(Value::as_str))?;
    Some(CfFile {
        id,
        display_name: mod_meta::clean(name, 64)?,
        file_date: v.get("fileDate").and_then(Value::as_str).and_then(|d| mod_meta::clean(d, 40)).unwrap_or_default(),
        url: v.get("url").and_then(Value::as_str).and_then(curseforge_page),
    })
}

fn parse_dep(v: &Value) -> Option<CfDep> {
    let slug = v.get("slug").and_then(Value::as_str).and_then(|s| mod_meta::clean(s, 64)).unwrap_or_default();
    let name = v.get("name").and_then(Value::as_str).and_then(|s| mod_meta::clean(s, mod_meta::MAX_NAME)).unwrap_or_else(|| slug.clone());
    if name.is_empty() {
        return None;
    }
    Some(CfDep { mod_id: v.get("modId")?.as_u64()?, name, slug, website: v.get("websiteUrl").and_then(Value::as_str).and_then(curseforge_page) })
}

fn parse_game_versions(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter_map(|s| mod_meta::clean(s, 32)).take(MAX_GAME_VERSIONS).collect())
        .unwrap_or_default()
}

fn parse_match(v: &Value) -> Option<CfMatch> {
    let fingerprints: Vec<u32> = v
        .get("fingerprints")?
        .as_array()?
        .iter()
        .filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()))
        .take(MAX_PER_REQUEST)
        .collect();
    if fingerprints.is_empty() {
        return None;
    }
    let mut authors: Vec<String> = v
        .get("authors")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter_map(|s| mod_meta::clean(s, 64)).collect())
        .unwrap_or_default();
    authors.truncate(mod_meta::MAX_AUTHORS);
    Some(CfMatch {
        fingerprints,
        mod_id: v.get("modId")?.as_u64()?,
        name: v.get("name").and_then(Value::as_str).and_then(|s| mod_meta::clean(s, mod_meta::MAX_NAME))?,
        authors,
        summary: v.get("summary").and_then(Value::as_str).and_then(mod_meta::clean_desc),
        website: v.get("websiteUrl").and_then(Value::as_str).and_then(mod_meta::clean_url),
        logo: v.get("logo").and_then(Value::as_str).and_then(logo_url),
        file: parse_file(v.get("file")),
        latest: parse_file(v.get("latest")),
        requires: v.get("requires").and_then(Value::as_array).map(|a| a.iter().filter_map(parse_dep).take(MAX_DEPS).collect()).unwrap_or_default(),
        game_versions: parse_game_versions(v.get("gameVersions")),
    })
}

/// A short reason for the report ("CurseForge: <reason>"). The proxy's own
/// messages are written for people; a bare status is not.
pub fn friendly_error(status: u16, server: Option<&str>) -> String {
    let server = server.and_then(|s| mod_meta::clean(s, 160)).map(|s| s.trim_end_matches('.').to_string()).filter(|s| !s.is_empty());
    match (status, server) {
        (429, _) => "too many checks right now, try again in a few minutes".into(),
        (502 | 504, _) | (500..=599, None) => "CurseForge didn't answer, try again in a minute".into(),
        (_, Some(msg)) => msg,
        _ => format!("HTTP {status}"),
    }
}

/// The proxy's answer: its matches, or a reason it refused.
pub fn parse_response(status: u16, json: Option<&Value>) -> Result<Vec<CfMatch>, String> {
    let server_error = json.and_then(|j| j.get("error")).and_then(Value::as_str);
    if !(200..300).contains(&status) || server_error.is_some() {
        return Err(friendly_error(status, server_error));
    }
    let matches = json.and_then(|j| j.get("matches")).and_then(Value::as_array).ok_or("unexpected response")?;
    Ok(matches.iter().take(MAX_MATCHES).filter_map(parse_match).collect())
}

// ---------------------------------------------------------------------------
// Network

/// What a batched lookup found before it stopped.
#[derive(Debug, Default)]
pub struct Lookup {
    pub matches: Vec<CfMatch>,
    /// How many of the fingerprints (from the start) got an answer.
    pub answered: usize,
    /// Why the rest weren't asked, if a request failed.
    pub error: Option<String>,
}

/// Runs `fetch` over `fingerprints` in proxy-sized chunks. A failure stops
/// the rest (the next request would most likely fail the same way) but keeps
/// what the earlier chunks found: on a 20k-file Sims folder, a rate limit on
/// the last chunk used to throw away thousands of answers.
async fn lookup_chunks<'a, F, Fut>(fingerprints: &'a [u32], mut fetch: F) -> Lookup
where
    F: FnMut(&'a [u32]) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<CfMatch>, String>>,
{
    let mut out = Lookup::default();
    for chunk in fingerprints.chunks(MAX_PER_REQUEST) {
        match fetch(chunk).await {
            Ok(m) => {
                out.matches.extend(m);
                out.answered += chunk.len();
            }
            Err(e) => {
                out.error = Some(e);
                break;
            }
        }
    }
    out
}

/// What the proxy is asked: WoW clients share CurseForge's one WoW game, and
/// the flavour picks the updates built for this client.
fn request_body(cf: &CurseForgeSupport, chunk: &[u32]) -> Value {
    match &cf.flavor {
        Some(flavor) => serde_json::json!({ "game": cf.game, "flavor": flavor, "fingerprints": chunk }),
        None => serde_json::json!({ "game": cf.game, "fingerprints": chunk }),
    }
}

async fn fetch_chunk(http: &reqwest::Client, cf: &CurseForgeSupport, chunk: &[u32]) -> Result<Vec<CfMatch>, String> {
    let body = request_body(cf, chunk);
    let resp = http.post(LOOKUP_URL).timeout(REQUEST_TIMEOUT).json(&body).send().await;
    let resp = resp.map_err(|e| if e.is_timeout() { "timed out".to_string() } else { "couldn't reach synccrate.app".to_string() })?;
    let status = resp.status().as_u16();
    let bytes = crate::commands::art::read_capped(resp, MAX_RESPONSE_BYTES).await.map_err(|_| "unexpected response".to_string())?;
    let json: Option<Value> = serde_json::from_slice(&bytes).ok();
    parse_response(status, json.as_ref())
}

/// Ask the proxy about `fingerprints` (batched to its 1000-per-request limit).
pub async fn lookup(cf: &CurseForgeSupport, fingerprints: &[u32]) -> Result<Lookup, String> {
    let http = crate::mod_updates::client()?;
    Ok(lookup_chunks(fingerprints, |chunk| fetch_chunk(&http, cf, chunk)).await)
}

fn logo_mime(bytes: &[u8]) -> Option<&'static str> {
    mod_meta::image_mime(bytes).or_else(|| {
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some("image/gif")
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some("image/webp")
        } else {
            None
        }
    })
}

/// A CurseForge logo as a `data:` URL, downloaded now and kept nowhere.
pub async fn logo_data_url(url: &str) -> Option<String> {
    use base64::Engine as _;
    let url = logo_url(url)?;
    let resp = crate::mod_updates::client().ok()?.get(url).timeout(Duration::from_secs(15)).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = crate::commands::art::read_capped(resp, MAX_LOGO_BYTES).await.ok()?;
    let mime = logo_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

// ---------------------------------------------------------------------------
// Results

#[derive(Debug, Default)]
pub struct Outcome {
    pub updates: Vec<ModUpdate>,
    /// Metadata for every matched file (one each, so also the files
    /// CurseForge recognized), merged with what it already had.
    pub metas: Vec<ModMeta>,
    /// Missing dependencies and (Minecraft) jars made for another setup.
    pub warnings: Vec<ModWarning>,
    /// Why not every file was looked up, if so.
    pub note: Option<String>,
}

/// `(seconds part, fraction padded to 9 digits)` of an ISO 8601 UTC date, so
/// "…:56Z" and "…:56.1Z" compare by time, not by text.
fn date_key(s: &str) -> Option<(&str, String)> {
    let secs = s.get(..19)?;
    let shape_ok = secs.bytes().enumerate().all(|(i, b)| match i {
        4 | 7 => b == b'-',
        10 => b == b'T',
        13 | 16 => b == b':',
        _ => b.is_ascii_digit(),
    });
    if !shape_ok {
        return None;
    }
    let frac: String = s[19..].strip_prefix('.').unwrap_or("").chars().take_while(char::is_ascii_digit).take(9).collect();
    Some((secs, format!("{frac:0<9}")))
}

/// (current, latest) display names when the mod has a newer file than the
/// one installed: a different file that is also more recent.
pub fn update_of(m: &CfMatch) -> Option<(String, String)> {
    let (cur, new) = (m.file.as_ref()?, m.latest.as_ref()?);
    let newer = new.id != cur.id && date_key(&new.file_date)? > date_key(&cur.file_date)?;
    newer.then(|| (cur.display_name.clone(), new.display_name.clone()))
}

/// What a matched file shows. A file with its own metadata keeps its name and
/// picture (they describe this exact file) and only gains what it lacks; a
/// name made up from the file name gives way to the real one.
pub fn merge_meta(offline: Option<&ModMeta>, key: &str, m: &CfMatch, is_file: bool) -> ModMeta {
    let mut meta = match offline {
        Some(o) => {
            let mut meta = o.clone();
            if meta.derived_name {
                meta.name = m.name.clone();
                meta.derived_name = false;
            }
            if meta.website.is_none() {
                meta.website = m.website.clone();
            }
            if meta.authors.is_empty() {
                meta.authors = m.authors.clone();
            }
            meta
        }
        None => ModMeta {
            key: key.to_string(),
            is_file,
            source: "curseforge".into(),
            id: Some(m.mod_id.to_string()),
            name: m.name.clone(),
            authors: m.authors.clone(),
            description: m.summary.clone(),
            website: m.website.clone(),
            ..Default::default()
        },
    };
    if !meta.has_icon {
        if let Some(logo) = &m.logo {
            meta.icon = IconRef::Remote(logo.clone());
            meta.has_icon = true;
        }
    }
    meta.curseforge = true;
    meta
}

fn is_disabled(key: &str) -> bool {
    key.to_ascii_lowercase().ends_with(crate::commands::files::DISABLED_SUFFIX)
}

/// Lower case without spaces, dashes and underscores, so "Fabric API",
/// "fabric-api" and "fabric_api" are the same name.
pub fn norm_name(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).flat_map(char::to_lowercase).collect()
}

/// What counts as installed when looking for a mod's dependencies: mods
/// CurseForge recognised, plus every installed mod's own ids and names (a
/// dependency can be installed from Modrinth or by hand, and then CurseForge
/// doesn't know the file).
#[derive(Debug, Default)]
pub struct Installed {
    mod_ids: std::collections::HashSet<u64>,
    names: std::collections::HashSet<String>,
}

impl Installed {
    pub fn new(cf_mod_ids: impl IntoIterator<Item = u64>, metas: &[ModMeta]) -> Self {
        let mut names = std::collections::HashSet::new();
        for m in metas.iter().filter(|m| !is_disabled(&m.key)) {
            if !m.derived_name {
                names.insert(norm_name(&m.name));
            }
            if let Some(id) = &m.id {
                names.insert(norm_name(id));
                // Thunderstore "Author-Name" and SMAPI "Author.Name": the
                // name part is what CurseForge's slug would be.
                if let Some((_, last)) = id.rsplit_once(['-', '.']) {
                    names.insert(norm_name(last));
                }
            }
        }
        names.remove("");
        Installed { mod_ids: cf_mod_ids.into_iter().collect(), names }
    }

    pub fn has(&self, d: &CfDep) -> bool {
        self.mod_ids.contains(&d.mod_id) || [&d.slug, &d.name].iter().any(|s| self.names.contains(&norm_name(s)))
    }
}

/// "Needs <name>" for each enabled file whose required dependencies aren't
/// installed. `matched` pairs file keys with their match.
pub fn missing_dependencies(matched: &[(&str, &CfMatch)], metas: &[ModMeta]) -> Vec<ModWarning> {
    let live: Vec<&(&str, &CfMatch)> = matched.iter().filter(|(k, _)| !is_disabled(k)).collect();
    let installed = Installed::new(live.iter().map(|(_, m)| m.mod_id), metas);
    let mut out = Vec::new();
    for (key, m) in live {
        let mut seen = std::collections::HashSet::new();
        for d in &m.requires {
            if d.mod_id != m.mod_id && seen.insert(d.mod_id) && !installed.has(d) {
                out.push(ModWarning {
                    key: key.to_string(),
                    kind: "missing_dependency".into(),
                    text: format!("Needs {}: not found in your mods", d.name),
                    url: d.website.clone(),
                });
            }
        }
    }
    out
}

const MC_LOADERS: [&str; 4] = ["Forge", "NeoForge", "Fabric", "Quilt"];

fn mc_loader(v: &str) -> Option<&'static str> {
    MC_LOADERS.iter().copied().find(|l| l.eq_ignore_ascii_case(v))
}

/// "1.20.1" or "1.21", not "1.20-Snapshot" or "Java 17".
fn is_mc_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn highest(versions: &[String]) -> Option<String> {
    let mut best: Option<&str> = None;
    for v in versions {
        if best.map_or(true, |b| crate::mod_updates::is_newer(v, b)) {
            best = Some(v);
        }
    }
    best.map(str::to_string)
}

/// The values more than half of `sets` (non-empty ones only) include, when
/// there are at least 3. Ties at the top all count: jars usually list
/// several versions ("1.20", "1.20.1"), and picking one of two equally
/// common ones would flag jars that only list the other.
fn majority(sets: &[Vec<String>]) -> Vec<String> {
    let sets: Vec<&Vec<String>> = sets.iter().filter(|s| !s.is_empty()).collect();
    if sets.len() < 3 {
        return Vec::new();
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for s in &sets {
        for v in s.iter().collect::<std::collections::HashSet<_>>() {
            *counts.entry(v.as_str()).or_default() += 1;
        }
    }
    let top = counts.values().copied().max().unwrap_or(0);
    if top * 2 <= sets.len() {
        return Vec::new();
    }
    let mut out: Vec<String> = counts.into_iter().filter(|(_, n)| *n == top).map(|(v, _)| v.to_string()).collect();
    out.sort();
    out
}

/// Whether a jar for `jar` loaders runs where most mods use `most`. Quilt
/// loads Fabric mods, so a Quilt jar among Fabric ones may just mean the
/// player uses Quilt; on 1.20.1 NeoForge still loads Forge mods.
fn loaders_fit(jar: &[String], most: &[String], mc: &[String]) -> bool {
    let has = |set: &[String], l: &str| set.iter().any(|x| x == l);
    let fits = |a: &str, b: &str| has(jar, a) && has(most, b);
    jar.iter().any(|l| has(most, l))
        || fits("Quilt", "Fabric")
        || fits("Fabric", "Quilt")
        || (has(mc, "1.20.1") && (fits("Forge", "NeoForge") || fits("NeoForge", "Forge")))
}

/// Minecraft jars made for another game version or loader than most of the
/// player's mods (`jars`: key and CurseForge's game versions for each
/// enabled matched jar). Needs 3+ jars and a clear majority, so a mixed or
/// small folder isn't second-guessed.
pub fn version_warnings(jars: &[(String, Vec<String>)]) -> Vec<ModWarning> {
    let versions: Vec<Vec<String>> = jars.iter().map(|(_, gv)| gv.iter().filter(|v| is_mc_version(v)).cloned().collect()).collect();
    let loaders: Vec<Vec<String>> = jars
        .iter()
        .map(|(_, gv)| MC_LOADERS.iter().filter(|l| gv.iter().any(|v| mc_loader(v) == Some(**l))).map(|l| l.to_string()).collect())
        .collect();
    let (most_mc, most_loader) = (majority(&versions), majority(&loaders));
    let mut out = Vec::new();
    for (i, (key, _)) in jars.iter().enumerate() {
        let mine = &versions[i];
        if !most_mc.is_empty() && !mine.is_empty() && !mine.iter().any(|v| most_mc.contains(v)) {
            if let (Some(made), Some(most)) = (highest(mine), highest(&most_mc)) {
                out.push(ModWarning { key: key.clone(), kind: "game_version".into(), text: format!("Made for {made}; most of your mods are for {most}"), url: None });
            }
        }
        let mine = &loaders[i];
        if !most_loader.is_empty() && !mine.is_empty() && !loaders_fit(mine, &most_loader, &most_mc) {
            // MC_LOADERS order, so ties name the same loader every time.
            let most = MC_LOADERS.iter().find(|l| most_loader.iter().any(|m| m == *l)).copied().unwrap_or_default();
            out.push(ModWarning { key: key.clone(), kind: "loader".into(), text: format!("A {} mod; most of your mods are {most}", mine.join("/")), url: None });
        }
    }
    out
}

/// Turn the proxy's matches into updates and metadata for the files whose
/// fingerprints they list, plus warnings about missing dependencies and
/// (Minecraft) jars for another setup. `whole_folder` says every selected
/// file was looked up: only then can a dependency be called missing, since
/// on a partial check it may just be among the files nobody asked about.
pub fn apply(cf: &CurseForgeSupport, prints: &[(String, u32)], matches: &[CfMatch], offline: &[ModMeta], whole_folder: bool) -> Outcome {
    let by_print: HashMap<u32, &CfMatch> = matches.iter().flat_map(|m| m.fingerprints.iter().map(move |fp| (*fp, m))).collect();
    let by_key: HashMap<&str, &ModMeta> = offline.iter().map(|m| (m.key.as_str(), m)).collect();
    // WoW keys are addon folders, and one addon often has several (DBM has
    // nine): each folder gets the names, but the update and the warnings
    // show once per addon, on its first folder.
    let folders = by_folder(cf);
    let mut out = Outcome::default();
    let mut matched: Vec<(&str, &CfMatch)> = Vec::new();
    let mut seen_mods = std::collections::HashSet::new();
    for (key, fp) in prints {
        let Some(m) = by_print.get(fp) else { continue };
        out.metas.push(merge_meta(by_key.get(key.as_str()).copied(), key, m, !folders));
        if folders && !seen_mods.insert(m.mod_id) {
            continue;
        }
        matched.push((key, m));
        if let Some((current, latest)) = update_of(m) {
            let changelog = m.latest.as_ref().and_then(|l| l.url.clone());
            out.updates.push(ModUpdate { key: key.clone(), source: "curseforge".into(), current: Some(current), latest, url: m.website.clone(), deprecated: false, changelog });
        }
    }
    if whole_folder {
        out.warnings = missing_dependencies(&matched, offline);
    }
    if cf.game == "minecraft_java" {
        let jars: Vec<(String, Vec<String>)> = matched.iter().filter(|(k, _)| !is_disabled(k)).map(|(k, m)| (k.to_string(), m.game_versions.clone())).collect();
        out.warnings.extend(version_warnings(&jars));
    }
    out
}

/// Fingerprint `files` (from `select_files`), ask the proxy and merge the
/// answers with the files' own metadata (`offline`).
pub async fn check(cf: &CurseForgeSupport, base: &str, files: &[String], offline: &[ModMeta]) -> Result<Outcome, String> {
    let b = base.to_string();
    let (prints, finished, total, unit) = if by_folder(cf) {
        let folders = addon_folders(files);
        let total = folders.len();
        let folders: Vec<(String, Vec<String>)> = folders.into_iter().take(MAX_FILES).collect();
        let (prints, finished) = tokio::task::spawn_blocking(move || fingerprint_addon_folders(&b, &folders)).await.map_err(|e| e.to_string())?;
        (prints, finished, total, "addon folders")
    } else {
        let total = files.len();
        let files: Vec<String> = files.iter().take(MAX_FILES).cloned().collect();
        let (prints, finished) = tokio::task::spawn_blocking(move || fingerprint_files(&b, &files)).await.map_err(|e| e.to_string())?;
        (prints, finished, total, "files")
    };
    let mut fps: Vec<u32> = prints.iter().map(|(_, fp)| *fp).collect();
    fps.sort_unstable();
    fps.dedup();
    let found = if fps.is_empty() { Lookup::default() } else { lookup(cf, &fps).await? };
    if found.answered == 0 {
        if let Some(e) = found.error {
            return Err(e);
        }
    }
    // The note is there exactly when part of the folder went unasked (cap,
    // time budget or a failed chunk).
    let note = coverage_note(&prints, &fps[..found.answered], found.error.as_deref(), finished, total, unit);
    let mut out = apply(cf, &prints, &found.matches, offline, note.is_none());
    out.note = note;
    Ok(out)
}

/// The report line when not every file was looked up. `answered` is the
/// sorted part of the fingerprints that got a reply before `error`.
fn coverage_note(prints: &[(String, u32)], answered: &[u32], error: Option<&str>, finished: bool, total: usize, unit: &str) -> Option<String> {
    if let Some(e) = error {
        let n = prints.iter().filter(|(_, fp)| answered.binary_search(fp).is_ok()).count();
        return Some(format!("checked {n} of {total} {unit}, then: {e}"));
    }
    (!finished || total > MAX_FILES).then(|| format!("checked {} of {total} {unit} (big folders take a while; try again for the rest)", prints.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry's CurseForge entry for a game.
    fn cf(id: &str) -> CurseForgeSupport {
        cf_of(id).unwrap_or_else(|| panic!("{id} isn't on CurseForge"))
    }

    fn cf_of(id: &str) -> Option<CurseForgeSupport> {
        crate::registry::load_registry().games.into_iter().find(|g| g.id == id).and_then(|g| g.curseforge)
    }

    // Reference values from a straight MurmurHash2 (seed 1, whitespace bytes
    // dropped, length = bytes kept), checked against a separate implementation.
    #[test]
    fn fingerprint_vectors() {
        assert_eq!(fingerprint(b""), 1540447798);
        assert_eq!(fingerprint(b"a"), 626045324, "1 tail byte");
        assert_eq!(fingerprint(b"ab"), 1692487918, "2 tail bytes");
        assert_eq!(fingerprint(b"abc"), 1621425345, "3 tail bytes");
        assert_eq!(fingerprint(b"abcd"), 3376380438, "no tail");
        assert_eq!(fingerprint(b"Hello, World!"), 1961219979);
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(fingerprint(&all), 2094645347);
    }

    #[test]
    fn fingerprint_ignores_whitespace_bytes() {
        assert_eq!(fingerprint(b"hello world\r\n\tfoo bar"), 3868846180);
        assert_eq!(fingerprint(b"helloworldfoobar"), 3868846180);
        assert_eq!(fingerprint(b" \t\r\n"), fingerprint(b""), "only whitespace is the empty file");
        assert_ne!(fingerprint(b"a\x0bb"), fingerprint(b"ab"), "only tab, LF, CR and space are dropped");
    }

    #[test]
    fn streaming_matches_one_shot() {
        // Pseudo-random bytes with plenty of whitespace values among them.
        let mut x = 0x1234_5678u32;
        let data: Vec<u8> = (0..10_007)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if x % 7 == 0 { b' ' } else { x as u8 }
            })
            .collect();
        let want = fingerprint(&data);
        for buf in [1, 3, 4, 7, 1000, 1 << 20] {
            let mut c = std::io::Cursor::new(&data);
            assert_eq!(fingerprint_stream(&mut c, buf).unwrap(), want, "buffer {buf}");
        }
        let mut c = std::io::Cursor::new(b"hello world\r\n\tfoo bar".to_vec());
        assert_eq!(fingerprint_stream(&mut c, 2).unwrap(), 3868846180);
    }

    #[test]
    fn fingerprints_files_on_disk() {
        let dir = crate::testutil::temp_dir("curseforge");
        crate::testutil::write_file(&dir, "mods/a.jar", b"abc");
        let (prints, finished) = fingerprint_files(dir.to_str().unwrap(), &["mods/a.jar".into(), "mods/gone.jar".into(), "../x.jar".into()]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(finished);
        assert_eq!(prints, vec![("mods/a.jar".to_string(), 1621425345)]);
    }

    #[test]
    fn file_selection_per_game() {
        let files: Vec<String> = [
            "Mods/CC/hair.package",
            "Mods/Script.ts4script",
            "Mods/old.package.disabled",
            "Mods/readme.txt",
            "Mods/x.zip",
            "Tray/house.package",
            "mods/sodium.jar",
            "mods/old.jar.disabled",
            "mods/config.toml",
            "resourcepacks/pack.zip",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(select_files(&cf("sims4"), "Mods", &files), vec!["Mods/CC/hair.package", "Mods/Script.ts4script", "Mods/old.package.disabled"]);
        // The registry's folder spelling, matched like the manifest's (case-insensitively).
        assert_eq!(select_files(&cf("minecraft_java"), "mods", &files), vec!["mods/old.jar.disabled", "mods/sodium.jar"]);
        assert!(cf_of("stardew_valley").is_none(), "only the games the proxy knows");
        assert!(select_files(&cf("wow_retail"), "WTF", &files).is_empty(), "WoW addons only");
    }

    fn sample() -> Value {
        serde_json::json!({"matches": [
            {"fingerprints": [111, 222], "modId": 238222, "name": "Just Enough Items", "authors": ["mezz"], "summary": "View items",
             "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
             "logo": "https://media.forgecdn.net/avatars/thumbnails/29/69/256/256/635838945588716414.jpeg",
             "file": {"id": 1, "displayName": "jei-1.20.1-15.2.0", "fileName": "jei.jar", "fileDate": "2023-08-01T10:00:00Z"},
             "latest": {"id": 2, "displayName": "jei-1.20.1-15.3.0", "fileName": "jei.jar", "fileDate": "2023-09-01T10:00:00.5Z",
                        "url": "https://www.curseforge.com/minecraft/mc-mods/jei/files/2"},
             "gameVersions": ["1.20.1", "Forge", "Client"],
             "requires": [{"modId": 306612, "name": "Fabric API", "slug": "fabric-api", "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/fabric-api"},
                          {"modId": 7, "name": "Bad link", "slug": "bad", "websiteUrl": "https://evil.example/x"},
                          {"name": "No id"}]},
            {"fingerprints": [333], "modId": 5, "name": "Up To Date", "authors": [], "summary": "", "websiteUrl": "javascript:alert(1)",
             "logo": "https://evil.example/x.png",
             "file": {"id": 9, "displayName": "v1", "fileName": "a.package", "fileDate": "2024-01-01T00:00:00Z"},
             "latest": {"id": 9, "displayName": "v1", "fileName": "a.package", "fileDate": "2024-01-01T00:00:00Z", "url": "https://curseforge.com.evil.example/x"}},
            {"fingerprints": [], "modId": 6, "name": "No prints"},
            {"fingerprints": [444], "name": "No id"}
        ]})
    }

    #[test]
    fn response_parsing() {
        let m = parse_response(200, Some(&sample())).unwrap();
        assert_eq!(m.len(), 2, "matches without fingerprints or a mod id are dropped");
        assert_eq!((m[0].mod_id, m[0].name.as_str(), m[0].fingerprints.clone()), (238222, "Just Enough Items", vec![111, 222]));
        assert_eq!(m[0].logo.as_deref(), Some("https://media.forgecdn.net/avatars/thumbnails/29/69/256/256/635838945588716414.jpeg"));
        assert_eq!(m[0].latest.as_ref().unwrap().display_name, "jei-1.20.1-15.3.0");
        assert_eq!((m[1].website.clone(), m[1].logo.clone(), m[1].summary.clone()), (None, None, None), "non-https links and other hosts' images are dropped");
        assert_eq!(m[0].latest.as_ref().unwrap().url.as_deref(), Some("https://www.curseforge.com/minecraft/mc-mods/jei/files/2"));
        assert_eq!(m[1].latest.as_ref().unwrap().url, None, "file links only to curseforge.com");
        assert_eq!(m[0].game_versions, vec!["1.20.1", "Forge", "Client"]);
        assert_eq!(m[0].requires.len(), 2, "a dependency without a mod id is dropped");
        assert_eq!((m[0].requires[0].mod_id, m[0].requires[0].slug.as_str()), (306612, "fabric-api"));
        assert_eq!(m[0].requires[1].website, None);
    }

    #[test]
    fn error_responses_are_friendly() {
        let e = serde_json::json!({"error": "CurseForge didn't answer. Try again in a minute."});
        assert_eq!(parse_response(502, Some(&e)).unwrap_err(), "CurseForge didn't answer, try again in a minute");
        let e = serde_json::json!({"error": "That game isn't on CurseForge here."});
        assert_eq!(parse_response(400, Some(&e)).unwrap_err(), "That game isn't on CurseForge here");
        assert_eq!(parse_response(429, None).unwrap_err(), "too many checks right now, try again in a few minutes");
        assert_eq!(parse_response(500, None).unwrap_err(), "CurseForge didn't answer, try again in a minute");
        assert_eq!(parse_response(404, None).unwrap_err(), "HTTP 404");
        assert_eq!(parse_response(200, Some(&serde_json::json!({"oops": 1}))).unwrap_err(), "unexpected response");
        assert_eq!(parse_response(200, None).unwrap_err(), "unexpected response");
    }

    fn found(fp: u32) -> CfMatch {
        CfMatch { fingerprints: vec![fp], mod_id: fp as u64, name: format!("mod {fp}"), authors: vec![], summary: None, website: None, logo: None, file: None, latest: None, requires: vec![], game_versions: vec![] }
    }

    #[tokio::test]
    async fn a_failed_chunk_keeps_earlier_answers() {
        let fps: Vec<u32> = (0..2500).collect();
        let mut calls = 0;
        let got = lookup_chunks(&fps, |chunk| {
            calls += 1;
            let n = calls;
            let first = chunk[0];
            async move { if n == 3 { Err("too many checks right now".to_string()) } else { Ok(vec![found(first)]) } }
        })
        .await;
        assert_eq!(got.matches.iter().map(|m| m.mod_id).collect::<Vec<_>>(), vec![0, 1000]);
        assert_eq!(got.answered, 2000);
        assert_eq!(got.error.as_deref(), Some("too many checks right now"));

        // Each file counts once, by whether its fingerprint got an answer.
        let prints = vec![("a".to_string(), 5), ("b".to_string(), 5), ("c".to_string(), 1999), ("d".to_string(), 2400)];
        let note = coverage_note(&prints, &fps[..got.answered], got.error.as_deref(), true, 4, "files");
        assert_eq!(note.as_deref(), Some("checked 3 of 4 files, then: too many checks right now"));
        assert_eq!(coverage_note(&prints, &fps, None, true, 4, "files"), None);
        assert!(coverage_note(&prints, &fps, None, false, 9, "addon folders").unwrap().starts_with("checked 4 of 9 addon folders (big folders"));
    }

    #[tokio::test]
    async fn every_chunk_answering_has_no_error() {
        let fps: Vec<u32> = (0..1001).collect();
        let got = lookup_chunks(&fps, |chunk| {
            let first = chunk[0];
            async move { Ok(vec![found(first)]) }
        })
        .await;
        assert_eq!((got.matches.len(), got.answered, got.error), (2, 1001, None));
    }

    #[test]
    fn updates_need_a_newer_different_file() {
        let m = parse_response(200, Some(&sample())).unwrap();
        assert_eq!(update_of(&m[0]), Some(("jei-1.20.1-15.2.0".into(), "jei-1.20.1-15.3.0".into())));
        assert_eq!(update_of(&m[1]), None, "same file");
        let mut older = m[0].clone();
        older.latest.as_mut().unwrap().file_date = "2023-07-01T10:00:00Z".into();
        assert_eq!(update_of(&older), None, "a different but older file isn't an update");
        let mut same_second = m[0].clone();
        same_second.file.as_mut().unwrap().file_date = "2023-09-01T10:00:00Z".into();
        assert!(update_of(&same_second).is_some(), "fractions compare as time: .5 is later than none");
        let mut no_latest = m[0].clone();
        no_latest.latest = None;
        assert_eq!(update_of(&no_latest), None);
    }

    #[test]
    fn merging_into_metadata() {
        let matches = parse_response(200, Some(&sample())).unwrap();
        let offline = vec![
            // A jar with its own metadata and icon: keeps both.
            ModMeta { key: "mods/jei.jar".into(), is_file: true, source: "forge".into(), name: "JEI".into(), has_icon: true, icon: IconRef::JarEntry("logo.png".into()), ..Default::default() },
            // A package whose name came from its file name and has no picture.
            ModMeta { key: "Mods/a.package".into(), is_file: true, source: "sims4".into(), name: "a".into(), derived_name: true, ..Default::default() },
        ];
        let prints = vec![("mods/jei.jar".to_string(), 111), ("mods/jei-copy.jar".to_string(), 222), ("Mods/a.package".to_string(), 333), ("mods/unknown.jar".to_string(), 999)];
        let out = apply(&cf("minecraft_java"), &prints, &matches, &offline, true);
        assert_eq!(out.metas.len(), 3);
        let by_key: HashMap<&str, &ModMeta> = out.metas.iter().map(|m| (m.key.as_str(), m)).collect();
        let jei = by_key["mods/jei.jar"];
        assert_eq!((jei.name.as_str(), jei.source.as_str()), ("JEI", "forge"), "offline name kept");
        assert_eq!(jei.icon, IconRef::JarEntry("logo.png".into()), "offline icon kept");
        assert_eq!(jei.website.as_deref(), Some("https://www.curseforge.com/minecraft/mc-mods/jei"), "missing website added");
        assert_eq!(jei.authors, vec!["mezz".to_string()]);
        assert!(jei.curseforge);
        let copy = by_key["mods/jei-copy.jar"];
        assert_eq!((copy.name.as_str(), copy.source.as_str(), copy.id.as_deref()), ("Just Enough Items", "curseforge", Some("238222")));
        assert!(copy.is_file && copy.has_icon && matches!(copy.icon, IconRef::Remote(_)));
        let pkg = by_key["Mods/a.package"];
        assert_eq!((pkg.name.as_str(), pkg.derived_name, pkg.source.as_str()), ("Up To Date", false, "sims4"), "a made-up name gives way");
        assert!(!pkg.has_icon, "no logo from another host");
        assert!(!by_key.contains_key("mods/unknown.jar"));
        let keys: Vec<&str> = out.updates.iter().map(|u| u.key.as_str()).collect();
        assert_eq!(keys, vec!["mods/jei.jar", "mods/jei-copy.jar"]);
        assert_eq!(out.updates[0].source, "curseforge");
        assert_eq!(out.updates[0].url.as_deref(), Some("https://www.curseforge.com/minecraft/mc-mods/jei"));
        assert_eq!(out.updates[0].changelog.as_deref(), Some("https://www.curseforge.com/minecraft/mc-mods/jei/files/2"), "What's new");
        // JEI's sample needs Fabric API, which nothing installed provides.
        let needs: Vec<(&str, &str)> = out.warnings.iter().map(|w| (w.key.as_str(), w.text.as_str())).collect();
        assert_eq!(needs[0], ("mods/jei.jar", "Needs Fabric API: not found in your mods"));
        assert_eq!(out.warnings[0].url.as_deref(), Some("https://www.curseforge.com/minecraft/mc-mods/fabric-api"));
    }

    fn dep(mod_id: u64, name: &str, slug: &str) -> CfDep {
        CfDep { mod_id, name: name.into(), slug: slug.into(), website: None }
    }

    fn needing(mod_id: u64, deps: Vec<CfDep>) -> CfMatch {
        CfMatch { requires: deps, ..found(mod_id as u32) }
    }

    #[test]
    fn dependencies_count_as_installed_by_id_or_name() {
        assert_eq!(norm_name("Fabric API"), "fabricapi");
        assert_eq!(norm_name("fabric_api"), norm_name("fabric-api"));
        let metas = vec![
            ModMeta { key: "mods/cloth.jar".into(), is_file: true, source: "fabric".into(), id: Some("cloth-config".into()), name: "Cloth Config v13".into(), ..Default::default() },
            ModMeta { key: "mods/arch.jar".into(), is_file: true, source: "fabric".into(), id: Some("architectury".into()), name: "Architectury".into(), ..Default::default() },
            ModMeta { key: "mods/off.jar.disabled".into(), is_file: true, source: "fabric".into(), id: Some("geckolib".into()), name: "GeckoLib".into(), ..Default::default() },
            ModMeta { key: "BepInEx/plugins/x".into(), source: "thunderstore".into(), id: Some("ValheimModding-Jotunn".into()), name: "x".into(), ..Default::default() },
            ModMeta { key: "mods/made-up.jar".into(), is_file: true, name: "Iris".into(), derived_name: true, ..Default::default() },
        ];
        let installed = Installed::new([10], &metas);
        assert!(installed.has(&dep(10, "Anything", "anything")), "matched on CurseForge by mod id");
        assert!(installed.has(&dep(1, "Cloth Config API", "cloth-config")), "slug against a Fabric mod id");
        assert!(!installed.has(&dep(2, "Architectury API", "architectury-api")), "no partial matches");
        assert!(installed.has(&dep(2, "architectury", "architectury-api")), "name against a mod's name");
        assert!(installed.has(&dep(3, "Jotunn", "jotunn")), "the name part of a Thunderstore id");
        assert!(!installed.has(&dep(4, "GeckoLib", "geckolib")), "a disabled mod doesn't count");
        assert!(!installed.has(&dep(6, "Iris", "iris-shaders")), "a name made up from a file name doesn't count");
    }

    #[test]
    fn a_partial_check_names_no_missing_dependencies() {
        // b.jar may provide a.jar's Lib, but a capped or cut-short check may
        // never have looked it up: "Needs Lib" would be wrong.
        let a = needing(1, vec![dep(2, "Lib", "lib")]);
        let prints = vec![("mods/a.jar".to_string(), 1)];
        let whole = apply(&cf("minecraft_java"), &prints, &[a.clone()], &[], true);
        assert_eq!(texts(&whole.warnings), vec![("mods/a.jar", "Needs Lib: not found in your mods")]);
        let partial = apply(&cf("minecraft_java"), &prints, &[a], &[], false);
        assert!(partial.warnings.is_empty());
        assert_eq!(partial.metas.len(), 1, "names and updates still apply");
    }

    #[test]
    fn missing_dependencies_are_flagged_once_per_mod() {
        let a = needing(1, vec![dep(2, "Lib", "lib"), dep(2, "Lib", "lib"), dep(3, "Other", "other"), dep(1, "Itself", "self")]);
        let b = found(2);
        let off = needing(4, vec![dep(9, "Gone", "gone")]);
        let matched = vec![("mods/a.jar", &a), ("mods/b.jar", &b), ("mods/off.jar.disabled", &off)];
        let w = missing_dependencies(&matched, &[]);
        let got: Vec<(&str, &str)> = w.iter().map(|w| (w.key.as_str(), w.text.as_str())).collect();
        assert_eq!(got, vec![("mods/a.jar", "Needs Other: not found in your mods")], "b.jar provides Lib; disabled files aren't checked");
        // A disabled copy of the dependency doesn't satisfy it.
        let matched = vec![("mods/a.jar", &a), ("mods/b.jar.disabled", &b)];
        assert_eq!(missing_dependencies(&matched, &[]).len(), 2);
    }

    fn jar(key: &str, versions: &[&str]) -> (String, Vec<String>) {
        (key.to_string(), versions.iter().map(|s| s.to_string()).collect())
    }

    fn texts(w: &[ModWarning]) -> Vec<(&str, &str)> {
        w.iter().map(|w| (w.key.as_str(), w.text.as_str())).collect()
    }

    #[test]
    fn jars_for_another_minecraft_version_or_loader() {
        let jars = vec![
            jar("a", &["1.20", "1.20.1", "Fabric", "Quilt", "Client"]),
            jar("b", &["1.20.1", "Fabric"]),
            jar("c", &["1.20.1", "Fabric", "Server"]),
            jar("d", &["1.19.2", "1.19.1", "Forge"]),
            jar("e", &["1.20.1", "Quilt"]),
            jar("f", &[]),
        ];
        let w = version_warnings(&jars);
        assert_eq!(
            texts(&w),
            vec![("d", "Made for 1.19.2; most of your mods are for 1.20.1"), ("d", "A Forge mod; most of your mods are Fabric")],
            "Quilt loads Fabric mods; no versions listed, nothing to say"
        );
        assert_eq!(w[0].kind, "game_version");
        assert_eq!(w[1].kind, "loader");
    }

    #[test]
    fn no_version_warnings_without_a_clear_majority() {
        // Two jars: too few to say what "most" is.
        assert!(version_warnings(&[jar("a", &["1.20.1", "Fabric"]), jar("b", &["1.19.2", "Forge"])]).is_empty());
        // Half and half isn't a majority.
        let even = [jar("a", &["1.20.1", "Fabric"]), jar("b", &["1.20.1", "Fabric"]), jar("c", &["1.19.2", "Forge"]), jar("d", &["1.19.2", "Forge"])];
        assert!(version_warnings(&even).is_empty());
        // Tied top versions both count: a jar listing only "1.20" is fine.
        let tied = [jar("a", &["1.20", "1.20.1", "Fabric"]), jar("b", &["1.20", "1.20.1", "Fabric"]), jar("c", &["1.20", "Fabric"])];
        assert!(version_warnings(&tied).is_empty());
        // On 1.20.1 NeoForge still runs Forge mods.
        let neo = [jar("a", &["1.20.1", "NeoForge"]), jar("b", &["1.20.1", "NeoForge"]), jar("c", &["1.20.1", "Forge"])];
        assert!(version_warnings(&neo).is_empty());
        let neo21 = [jar("a", &["1.21.1", "NeoForge"]), jar("b", &["1.21.1", "NeoForge"]), jar("c", &["1.21.1", "Forge"])];
        assert_eq!(texts(&version_warnings(&neo21)), vec![("c", "A Forge mod; most of your mods are NeoForge")]);
        assert!(is_mc_version("1.21") && is_mc_version("1.20.1") && !is_mc_version("1.20-Snapshot") && !is_mc_version("Java 17"));
    }

    #[test]
    fn version_warnings_only_for_minecraft() {
        let m = |id: u32, gv: &[&str]| CfMatch { game_versions: gv.iter().map(|s| s.to_string()).collect(), ..found(id) };
        let ms = vec![m(1, &["1.20.1", "Fabric"]), m(2, &["1.20.1", "Fabric"]), m(3, &["1.20.1", "Fabric"]), m(4, &["1.19.2", "Fabric"])];
        let prints: Vec<(String, u32)> = (1..=4).map(|i| (format!("mods/{i}.jar"), i)).collect();
        assert_eq!(apply(&cf("minecraft_java"), &prints, &ms, &[], true).warnings.len(), 1);
        assert!(apply(&cf("sims4"), &prints, &ms, &[], true).warnings.is_empty());
    }

    // Checked against the live API: DBM-StatusBarTimers' .toc and DBT.lua,
    // and DBM-VPVEM's single file.
    #[test]
    fn folder_fingerprint_vectors() {
        assert_eq!(folder_fingerprint(&[2357310193, 737326275]), 1833201348, "sorted, so order doesn't matter");
        assert_eq!(folder_fingerprint(&[737326275, 2357310193]), 1833201348);
        assert_eq!(folder_fingerprint(&[3946803678]), 3933556927);
        assert_eq!(folder_fingerprint(&[3946803678]), fingerprint(b"3946803678"));
    }

    #[test]
    fn own_toc_files_take_any_suffix() {
        assert!(is_own_toc("DBM-Core", "DBM-Core.toc"));
        assert!(is_own_toc("DBM-Core", "dbm-core_Mainline.toc"));
        assert!(is_own_toc("QuestieDB", "QuestieDB_Forever.toc"), "Questie ships client names CurseForge's list lacks");
        assert!(is_own_toc("Questie", "Questie-Camelot.toc"));
        assert!(!is_own_toc("Questie", "QuestieDB.toc"), "another folder's name");
        assert!(!is_own_toc("Foo", "Libs/Foo.toc"), "only directly in the folder");
        assert!(!is_own_toc("Foo", "Foo.lua"));
    }

    #[test]
    fn toc_and_xml_includes() {
        let toc = "## Interface: 110200\n## Title: Foo\n# a comment.lua\nCore.lua\n  Libs\\Lib.xml  \nlocale.lua # trailing\nreadme.txt\n";
        assert_eq!(includes(toc, false), vec!["Core.lua", "Libs\\Lib.xml", "locale.lua"]);
        let xml = "<Ui>\n<!-- <Script file=\"old.lua\"/>\n still a comment -->\n<Script file=\"a.lua\"/>\n  <Include   FILE='sub\\b.xml' />\n<Script file=\"c.lua\"></Script>\n<Scriptfile=\"d.lua\"/>\n</Ui>";
        assert_eq!(includes(xml, true), vec!["a.lua", "sub\\b.xml"], "self-closing tags only, like CurseForge's own parser");
        assert_eq!(xml_includes(r#"<Frame/><Script file="x.lua"/>"#), vec!["x.lua"]);
    }

    #[test]
    fn compact_xml_loads_every_include_on_a_line() {
        let xml = concat!(
            r#"<Ui><Script file="a.lua"/><Include file="b.xml"/><!-- <Script file="old.lua"/> --><Script file='c.lua'/>"#,
            r#"<Frame name="x"/><Script file="d.lua"></Script><!-- <Include file="gone.xml"/>"#,
            "\n",
            r#"<Script file="gone.lua"/> --><Script"#,
            "\n",
            r#"  file="e.lua" /></Ui>"#,
        );
        assert_eq!(includes(xml, true), vec!["a.lua", "b.xml", "c.lua", "e.lua"], "document order; commented-out and non-self-closing tags skipped");
    }

    #[test]
    fn addon_load_list_follows_what_the_toc_loads() {
        let files: Vec<String> = [
            "Foo.toc", "Foo_Mainline.toc", "Bindings.xml", "core.lua", "Libs/libs.xml", "Libs/LibA/LibA.lua",
            "Libs/LibA/unused.lua", "Locales/enUS.lua", "Other.toc", "Media/icon.tga", "cycle.xml",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let read = |f: &str| -> Option<Vec<u8>> {
            let s = match f {
                "Foo.toc" => "## Title: Foo\nCore.lua\nLibs\\Libs.xml\nmissing.lua\n..\\Other\\x.lua\ncycle.xml\n",
                "Foo_Mainline.toc" => "Core.lua\nLocales/enUS.lua\n",
                "Libs/libs.xml" => "<Ui><Script file=\"LibA\\LibA.lua\"/></Ui>",
                "cycle.xml" => "<Include file=\"cycle.xml\"/>",
                "Bindings.xml" => "<Script file=\"Libs\\LibA\\unused.lua\"/>",
                _ => return Some(b"x".to_vec()),
            };
            Some(s.as_bytes().to_vec())
        };
        let mut got = addon_load_list("Foo", &files, &read);
        got.sort();
        assert_eq!(
            got,
            vec!["Bindings.xml", "Foo.toc", "Foo_Mainline.toc", "Libs/LibA/LibA.lua", "Libs/libs.xml", "Locales/enUS.lua", "core.lua", "cycle.xml"],
            "case-insensitive lookups, Bindings.xml hashed but not followed, no '..', each file once"
        );
        assert!(addon_load_list("Bar", &files, &read).is_empty(), "no .toc of its own, nothing to match");
    }

    #[test]
    fn addon_folders_fingerprint_from_disk() {
        let dir = crate::testutil::temp_dir("curseforge_wow");
        crate::testutil::write_file(&dir, "Interface/AddOns/DBM-VPVEM/DBM-VPVEM.toc", b"## Title: x\nsound.lua\n");
        crate::testutil::write_file(&dir, "Interface/AddOns/DBM-VPVEM/sound.lua", b"abc");
        crate::testutil::write_file(&dir, "Interface/AddOns/Leftover/readme.lua", b"abc");
        let files: Vec<String> = ["Interface/AddOns/DBM-VPVEM/DBM-VPVEM.toc", "Interface/AddOns/DBM-VPVEM/sound.lua", "Interface/AddOns/Leftover/readme.lua", "Interface/AddOns/stray.lua"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(select_files(&cf("wow_retail"), "Interface/AddOns", &files).len(), 4);
        assert!(cf_of("wow_wotlk").is_none(), "private-server clients aren't on CurseForge");
        let folders = addon_folders(&files);
        assert_eq!(folders.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), vec!["Interface/AddOns/DBM-VPVEM", "Interface/AddOns/Leftover"]);
        let (prints, finished) = fingerprint_addon_folders(dir.to_str().unwrap(), &folders);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(finished);
        let toc = fingerprint(b"## Title: x\nsound.lua\n");
        assert_eq!(prints, vec![("Interface/AddOns/DBM-VPVEM".to_string(), folder_fingerprint(&[toc, fingerprint(b"abc")]))]);
    }

    /// .toc and .xml prints come from the bytes read to follow them; they
    /// must equal what `fingerprint_file` gives, including a .toc past the
    /// one-shot size that the file path streams.
    #[test]
    fn loader_prints_from_read_bytes_match_the_file_path() {
        let dir = crate::testutil::temp_dir("curseforge_wow_loaders");
        let mut big = b"## Title: Big\r\nCore.lua\r\nLibs/libs.xml\r\n".to_vec();
        while (big.len() as u64) <= ONE_SHOT_BYTES {
            big.extend_from_slice(b"## X-Data: 0123456789 abcdef\t\r\n");
        }
        let base = "Interface/AddOns/Big";
        crate::testutil::write_file(&dir, &format!("{base}/Big.toc"), &big);
        crate::testutil::write_file(&dir, &format!("{base}/Core.lua"), b"print(1)\n");
        crate::testutil::write_file(&dir, &format!("{base}/Libs/libs.xml"), br#"<Ui> <Script file="a.lua"/><Script file="b.lua"/> </Ui>"#);
        crate::testutil::write_file(&dir, &format!("{base}/Libs/a.lua"), b"a");
        crate::testutil::write_file(&dir, &format!("{base}/Libs/b.lua"), b"b");
        let files: Vec<String> = ["Big.toc", "Core.lua", "Libs/libs.xml", "Libs/a.lua", "Libs/b.lua"].iter().map(|f| format!("{base}/{f}")).collect();
        let (prints, _) = fingerprint_addon_folders(dir.to_str().unwrap(), &addon_folders(&files));
        let by_file: Vec<u32> = files.iter().map(|f| fingerprint_file(&dir.join(f)).unwrap()).collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(by_file[0], fingerprint(&big));
        assert_eq!(prints, vec![(base.to_string(), folder_fingerprint(&by_file))]);
    }

    #[test]
    fn wow_asks_the_proxy_with_its_flavour() {
        assert_eq!(request_body(&cf("wow_classic_era"), &[1, 2]), serde_json::json!({"game": "wow", "flavor": "classic_era", "fingerprints": [1, 2]}));
        assert_eq!(request_body(&cf("wow_forever"), &[1])["flavor"], "forever");
        assert_eq!(request_body(&cf("sims4"), &[1]), serde_json::json!({"game": "sims4", "fingerprints": [1]}));
        assert_eq!(request_body(&cf("minecraft_java"), &[1]), serde_json::json!({"game": "minecraft_java", "fingerprints": [1]}));
    }

    #[test]
    fn a_wow_addon_with_several_folders_shows_its_update_once() {
        let mut dbm = found(1);
        dbm.fingerprints = vec![1, 2];
        dbm.file = Some(CfFile { id: 1, display_name: "12.1.11".into(), file_date: "2026-09-01T00:00:00Z".into(), url: None });
        dbm.latest = Some(CfFile { id: 2, display_name: "12.1.12".into(), file_date: "2026-10-01T00:00:00Z".into(), url: None });
        let prints = vec![("Interface/AddOns/DBM-Core".to_string(), 1), ("Interface/AddOns/DBM-GUI".to_string(), 2)];
        let offline = vec![ModMeta { key: "Interface/AddOns/DBM-Core".into(), source: "wow".into(), name: "DBM Core".into(), ..Default::default() }];
        let out = apply(&cf("wow_retail"), &prints, &[dbm], &offline, true);
        assert_eq!(out.metas.len(), 2, "both folders get CurseForge's info");
        assert_eq!((out.metas[0].name.as_str(), out.metas[0].curseforge), ("DBM Core", true), "the .toc's title stays");
        assert!(!out.metas[1].is_file, "a folder mod");
        assert_eq!(out.updates.iter().map(|u| u.key.as_str()).collect::<Vec<_>>(), vec!["Interface/AddOns/DBM-Core"]);
    }

    /// Prints folder fingerprints of real addons, to compare with the live
    /// API by hand: `SYNCCRATE_WOW_ADDONS=<dir with addon folders> cargo test wow_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn wow_live_fingerprints() {
        let Ok(root) = std::env::var("SYNCCRATE_WOW_ADDONS") else { return };
        let base = std::path::Path::new(&root);
        let mut files = Vec::new();
        fn walk(dir: &std::path::Path, rel: &str, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path(), &r, out);
                } else {
                    out.push(r);
                }
            }
        }
        walk(&base.join(WOW_ADDONS), WOW_ADDONS, &mut files);
        let selected = select_files(&cf("wow_retail"), WOW_ADDONS, &files);
        let (prints, _) = fingerprint_addon_folders(&root, &addon_folders(&selected));
        for (k, fp) in prints {
            println!("{k} {fp}");
        }
    }

    #[test]
    fn logo_hosts_and_types() {
        assert!(logo_url("https://media.forgecdn.net/avatars/1.png").is_some());
        assert!(logo_url("http://media.forgecdn.net/avatars/1.png").is_none());
        assert!(logo_url("https://forgecdn.net.evil.example/1.png").is_none());
        assert_eq!(logo_mime(b"GIF89a...."), Some("image/gif"));
        assert_eq!(logo_mime(b"<svg onload=alert(1)>"), None);
    }
}
