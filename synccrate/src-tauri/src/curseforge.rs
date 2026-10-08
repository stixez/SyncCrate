//! CurseForge names, pictures and updates for Sims 4 and Minecraft (Java).
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
use crate::mod_updates::ModUpdate;
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

/// Which scanned files CurseForge can know, for the games it's used for:
/// Sims 4 `.package` / `.ts4script` and Minecraft `.jar` files in the mods
/// content folder (`mods_folder`, as manifest paths spell it), disabled ones
/// included. Sorted, so a capped check always takes the same files.
pub fn select_files(game: &str, mods_folder: &str, files: &[String]) -> Vec<String> {
    let exts: &[&str] = match game {
        "sims4" => &["package", "ts4script"],
        "minecraft_java" => &["jar"],
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
// Response

#[derive(Debug, Clone, PartialEq)]
pub struct CfFile {
    pub id: u64,
    pub display_name: String,
    pub file_date: String,
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
    })
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

/// Ask the proxy about `fingerprints` (batched to its 1000-per-request limit).
pub async fn lookup(game: &str, fingerprints: &[u32]) -> Result<Vec<CfMatch>, String> {
    let http = crate::mod_updates::client()?;
    let mut out = Vec::new();
    for chunk in fingerprints.chunks(MAX_PER_REQUEST) {
        let body = serde_json::json!({ "game": game, "fingerprints": chunk });
        let resp = http.post(LOOKUP_URL).timeout(REQUEST_TIMEOUT).json(&body).send().await;
        let resp = resp.map_err(|e| if e.is_timeout() { "timed out".to_string() } else { "couldn't reach synccrate.app".to_string() })?;
        let status = resp.status().as_u16();
        let bytes = crate::commands::art::read_capped(resp, MAX_RESPONSE_BYTES).await.map_err(|_| "unexpected response".to_string())?;
        let json: Option<Value> = serde_json::from_slice(&bytes).ok();
        out.extend(parse_response(status, json.as_ref())?);
    }
    Ok(out)
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
    /// Metadata for every matched file, merged with what it already had.
    pub metas: Vec<ModMeta>,
    /// Files CurseForge recognized.
    pub matched: usize,
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
pub fn merge_meta(offline: Option<&ModMeta>, key: &str, m: &CfMatch) -> ModMeta {
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
            is_file: true,
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

/// Turn the proxy's matches into updates and metadata for the files whose
/// fingerprints they list.
pub fn apply(prints: &[(String, u32)], matches: &[CfMatch], offline: &[ModMeta]) -> Outcome {
    let by_print: HashMap<u32, &CfMatch> = matches.iter().flat_map(|m| m.fingerprints.iter().map(move |fp| (*fp, m))).collect();
    let offline: HashMap<&str, &ModMeta> = offline.iter().filter(|m| m.is_file).map(|m| (m.key.as_str(), m)).collect();
    let mut out = Outcome::default();
    for (key, fp) in prints {
        let Some(m) = by_print.get(fp) else { continue };
        out.matched += 1;
        out.metas.push(merge_meta(offline.get(key.as_str()).copied(), key, m));
        if let Some((current, latest)) = update_of(m) {
            out.updates.push(ModUpdate { key: key.clone(), source: "curseforge".into(), current: Some(current), latest, url: m.website.clone(), deprecated: false });
        }
    }
    out
}

/// Fingerprint `files` (from `select_files`), ask the proxy and merge the
/// answers with the files' own metadata (`offline`).
pub async fn check(game: &str, base: &str, files: &[String], offline: &[ModMeta]) -> Result<Outcome, String> {
    let total = files.len();
    let files: Vec<String> = files.iter().take(MAX_FILES).cloned().collect();
    let b = base.to_string();
    let (prints, finished) = tokio::task::spawn_blocking(move || fingerprint_files(&b, &files)).await.map_err(|e| e.to_string())?;
    let mut fps: Vec<u32> = prints.iter().map(|(_, fp)| *fp).collect();
    fps.sort_unstable();
    fps.dedup();
    let matches = if fps.is_empty() { Vec::new() } else { lookup(game, &fps).await? };
    let mut out = apply(&prints, &matches, offline);
    if !finished || total > MAX_FILES {
        out.note = Some(format!("checked {} of {total} files (big folders take a while; try again for the rest)", prints.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(select_files("sims4", "Mods", &files), vec!["Mods/CC/hair.package", "Mods/Script.ts4script", "Mods/old.package.disabled"]);
        // The registry's folder spelling, matched like the manifest's (case-insensitively).
        assert_eq!(select_files("minecraft_java", "mods", &files), vec!["mods/old.jar.disabled", "mods/sodium.jar"]);
        assert!(select_files("stardew_valley", "Mods", &files).is_empty(), "only the games the proxy knows");
    }

    fn sample() -> Value {
        serde_json::json!({"matches": [
            {"fingerprints": [111, 222], "modId": 238222, "name": "Just Enough Items", "authors": ["mezz"], "summary": "View items",
             "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
             "logo": "https://media.forgecdn.net/avatars/thumbnails/29/69/256/256/635838945588716414.jpeg",
             "file": {"id": 1, "displayName": "jei-1.20.1-15.2.0", "fileName": "jei.jar", "fileDate": "2023-08-01T10:00:00Z"},
             "latest": {"id": 2, "displayName": "jei-1.20.1-15.3.0", "fileName": "jei.jar", "fileDate": "2023-09-01T10:00:00.5Z"}},
            {"fingerprints": [333], "modId": 5, "name": "Up To Date", "authors": [], "summary": "", "websiteUrl": "javascript:alert(1)",
             "logo": "https://evil.example/x.png",
             "file": {"id": 9, "displayName": "v1", "fileName": "a.package", "fileDate": "2024-01-01T00:00:00Z"},
             "latest": {"id": 9, "displayName": "v1", "fileName": "a.package", "fileDate": "2024-01-01T00:00:00Z"}},
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
        let out = apply(&prints, &matches, &offline);
        assert_eq!(out.matched, 3);
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
