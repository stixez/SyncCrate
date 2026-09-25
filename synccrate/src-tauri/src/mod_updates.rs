//! "Update available" for mods, from the public APIs that need no key:
//!
//! - Modrinth (Minecraft jars): the jar's SHA-512 finds its exact version,
//!   then `version_files/update` returns the newest one for the same loaders
//!   and Minecraft versions.
//! - Thunderstore (BepInEx games): `api/experimental/package/<ns>/<name>/`
//!   returns the latest version of an `Author-Name` package.
//! - SMAPI's web API (Stardew Valley): the manifest's `UpdateKeys`
//!   (Nexus/GitHub/ModDrop/CurseForge ids) in one batch request, the same
//!   service SMAPI itself uses.
//!
//! CurseForge and Nexus proper need API keys, so they're not used. Checks
//! only run when the user asks (they send mod ids/hashes to those services).
//! Every response is untrusted: sizes are capped, versions and URLs cleaned,
//! URLs must be https.
use crate::mod_meta::ModMeta;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

const USER_AGENT: &str = concat!("SyncCrate/", env!("CARGO_PKG_VERSION"), " (github.com/stixez/SyncCrate)");
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Per check; a mods folder bigger than this gets its first N checked.
pub const MAX_MODS_PER_SOURCE: usize = 300;
const MAX_JAR_BYTES: u64 = 256 * 1024 * 1024;
const THUNDERSTORE_PARALLEL: usize = 6;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModUpdate {
    /// `ModMeta::key` of the mod.
    pub key: String,
    pub source: String,
    pub current: Option<String>,
    pub latest: String,
    pub url: Option<String>,
    /// Thunderstore: the package is deprecated (no longer maintained).
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateReport {
    pub updates: Vec<ModUpdate>,
    /// Mods whose source was asked (whether or not they have an update).
    pub checked: usize,
    /// Sources that failed, with a short reason ("Modrinth: timed out").
    pub errors: Vec<String>,
}

fn clean_version(s: &str) -> Option<String> {
    let v: String = s.chars().filter(|c| !c.is_control() && !crate::chat::is_bidi_control(*c)).take(64).collect();
    let v = v.trim().to_string();
    (!v.is_empty()).then_some(v)
}

fn clean_https(url: &str) -> Option<String> {
    let u = url.trim();
    let ok = u.len() <= 300 && u.to_ascii_lowercase().starts_with("https://") && !u.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| u.to_string())
}

/// Numeric-aware comparison of version strings ("1.10.0" > "1.9.2",
/// "v2.0" == "2.0", "0.5.13-fabric" > "0.4.10"). Non-numeric parts compare
/// as text. Only says "newer" when the versions really differ.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<(u64, String)> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split(|c: char| c == '.' || c == '-' || c == '+' || c == '_')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                (digits.parse().unwrap_or(0), p[digits.len()..].to_ascii_lowercase())
            })
            .collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).cloned().unwrap_or((0, String::new()));
        let y = b.get(i).cloned().unwrap_or((0, String::new()));
        if x.0 != y.0 {
            return x.0 > y.0;
        }
        if x.1 != y.1 {
            // "1.0" vs "1.0-beta": a suffix is a pre-release of the bare version.
            if x.1.is_empty() || y.1.is_empty() {
                return x.1.is_empty() && !y.1.is_empty() && i > 0;
            }
            return x.1 > y.1;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Thunderstore

/// `Author-Name` from a Thunderstore manifest's id (the install folder name).
pub fn thunderstore_target(meta: &ModMeta) -> Option<(String, String)> {
    if meta.source != "thunderstore" {
        return None;
    }
    let (ns, name) = meta.id.as_deref()?.split_once('-')?;
    let ok = |s: &str| !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (ok(ns) && ok(name)).then(|| (ns.to_string(), name.to_string()))
}

/// (latest version, package page, deprecated) from the experimental package API.
pub fn parse_thunderstore(json: &Value) -> Option<(String, Option<String>, bool)> {
    let latest = json.get("latest")?;
    let version = latest.get("version_number")?.as_str().and_then(clean_version)?;
    let url = json.get("package_url").and_then(Value::as_str).and_then(clean_https);
    let deprecated = json.get("is_deprecated").and_then(Value::as_bool).unwrap_or(false);
    Some((version, url, deprecated))
}

// ---------------------------------------------------------------------------
// SMAPI

pub fn smapi_request(metas: &[&ModMeta], game_version: Option<&str>) -> Value {
    let mods: Vec<Value> = metas
        .iter()
        .take(MAX_MODS_PER_SOURCE)
        .filter_map(|m| {
            let id = m.id.as_deref()?;
            Some(serde_json::json!({ "id": id, "updateKeys": m.update_keys, "installedVersion": m.version.as_deref().unwrap_or("0.0.0") }))
        })
        .collect();
    serde_json::json!({
        "mods": mods,
        "apiVersion": "4.0.0",
        "gameVersion": game_version.unwrap_or("1.6.0"),
        "platform": if cfg!(target_os = "windows") { "Windows" } else if cfg!(target_os = "macos") { "Mac" } else { "Linux" },
        "includeExtendedMetadata": false,
    })
}

/// id → (suggested version, url). Mods with no suggestion are left out.
pub fn parse_smapi(json: &Value) -> HashMap<String, (String, Option<String>)> {
    let mut out = HashMap::new();
    for item in json.as_array().into_iter().flatten().take(MAX_MODS_PER_SOURCE) {
        let (Some(id), Some(s)) = (item.get("id").and_then(Value::as_str), item.get("suggestedUpdate")) else { continue };
        if let Some(v) = s.get("version").and_then(Value::as_str).and_then(clean_version) {
            out.insert(id.to_string(), (v, s.get("url").and_then(Value::as_str).and_then(clean_https)));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Modrinth

#[derive(Debug, Clone, PartialEq)]
pub struct MrVersion {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
}

fn strings(v: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = v
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|s| s.len() <= 64).map(str::to_string).take(64).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn is_mr_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// hash → version, from `version_files` or `version_files/update` (same shape).
pub fn parse_modrinth(json: &Value) -> HashMap<String, MrVersion> {
    let mut out = HashMap::new();
    let Some(map) = json.as_object() else { return out };
    for (hash, v) in map.iter().take(MAX_MODS_PER_SOURCE * 2) {
        let (Some(id), Some(project), Some(num)) = (
            v.get("id").and_then(Value::as_str).filter(|s| is_mr_id(s)),
            v.get("project_id").and_then(Value::as_str).filter(|s| is_mr_id(s)),
            v.get("version_number").and_then(Value::as_str).and_then(clean_version),
        ) else {
            continue;
        };
        out.insert(
            hash.clone(),
            MrVersion { id: id.into(), project_id: project.into(), version_number: num, loaders: strings(v.get("loaders")), game_versions: strings(v.get("game_versions")) },
        );
    }
    out
}

/// Hashes grouped by the (loaders, game versions) filter their update
/// request needs: one request per group, usually just one for a modpack.
pub fn modrinth_groups(current: &HashMap<String, MrVersion>) -> Vec<((Vec<String>, Vec<String>), Vec<String>)> {
    let mut groups: HashMap<(Vec<String>, Vec<String>), Vec<String>> = HashMap::new();
    for (hash, v) in current {
        groups.entry((v.loaders.clone(), v.game_versions.clone())).or_default().push(hash.clone());
    }
    let mut out: Vec<_> = groups.into_iter().collect();
    out.sort();
    out
}

fn sha512_file(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_JAR_BYTES {
        return None;
    }
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(hex::encode(h.finalize()))
}

// ---------------------------------------------------------------------------
// Network

fn client() -> Result<reqwest::Client, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|a| if a.url().scheme() == "https" && a.previous().len() < 5 { a.follow() } else { a.stop() }))
        .build()
        .map_err(|e| e.to_string())
}

async fn json_of(resp: Result<reqwest::Response, reqwest::Error>) -> Result<Value, String> {
    let resp = resp.map_err(|e| if e.is_timeout() { "timed out".to_string() } else { "couldn't connect".to_string() })?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let bytes = crate::commands::art::read_capped(resp, MAX_RESPONSE_BYTES).await?;
    serde_json::from_slice(&bytes).map_err(|_| "unexpected response".to_string())
}

async fn check_modrinth(http: &reqwest::Client, base: &str, metas: &[&ModMeta], report: &mut UpdateReport) -> Result<(), String> {
    let jars: Vec<(String, String)> = {
        let base = base.to_string();
        let keys: Vec<String> = metas.iter().filter(|m| m.is_file).take(MAX_MODS_PER_SOURCE).map(|m| m.key.clone()).collect();
        tokio::task::spawn_blocking(move || {
            keys.into_iter()
                .filter_map(|k| {
                    let path = crate::utils::safe_join(&base, &k).ok()?;
                    Some((sha512_file(&path)?, k))
                })
                .collect()
        })
        .await
        .map_err(|e| e.to_string())?
    };
    if jars.is_empty() {
        return Ok(());
    }
    let key_of: HashMap<&str, &str> = jars.iter().map(|(h, k)| (h.as_str(), k.as_str())).collect();
    let hashes: Vec<&str> = jars.iter().map(|(h, _)| h.as_str()).collect();
    let current = parse_modrinth(
        &json_of(http.post("https://api.modrinth.com/v2/version_files").json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha512" })).send().await).await?,
    );
    report.checked += current.len();
    for ((loaders, game_versions), group) in modrinth_groups(&current) {
        let body = serde_json::json!({ "hashes": group, "algorithm": "sha512", "loaders": loaders, "game_versions": game_versions });
        let latest = parse_modrinth(&json_of(http.post("https://api.modrinth.com/v2/version_files/update").json(&body).send().await).await?);
        for hash in &group {
            let (Some(cur), Some(new), Some(key)) = (current.get(hash), latest.get(hash), key_of.get(hash.as_str())) else { continue };
            if new.id != cur.id && new.project_id == cur.project_id {
                report.updates.push(ModUpdate {
                    key: key.to_string(),
                    source: "modrinth".into(),
                    current: Some(cur.version_number.clone()),
                    latest: new.version_number.clone(),
                    url: Some(format!("https://modrinth.com/project/{}/version/{}", new.project_id, new.id)),
                    deprecated: false,
                });
            }
        }
    }
    Ok(())
}

async fn check_thunderstore(http: &reqwest::Client, metas: &[&ModMeta], report: &mut UpdateReport) -> Result<(), String> {
    let targets: Vec<(&ModMeta, String, String)> = metas.iter().filter_map(|m| thunderstore_target(m).map(|(ns, n)| (*m, ns, n))).take(MAX_MODS_PER_SOURCE).collect();
    let mut failures = 0;
    for chunk in targets.chunks(THUNDERSTORE_PARALLEL) {
        let mut set = tokio::task::JoinSet::new();
        for (i, (_, ns, name)) in chunk.iter().enumerate() {
            let (http, url) = (http.clone(), format!("https://thunderstore.io/api/experimental/package/{ns}/{name}/"));
            set.spawn(async move { (i, json_of(http.get(url).send().await).await) });
        }
        let mut results: Vec<Option<Result<Value, String>>> = (0..chunk.len()).map(|_| None).collect();
        while let Some(joined) = set.join_next().await {
            if let Ok((i, r)) = joined {
                results[i] = Some(r);
            }
        }
        for ((meta, _, _), result) in chunk.iter().zip(results) {
            let result = result.unwrap_or_else(|| Err("task failed".into()));
            let Ok(json) = result else {
                failures += 1;
                continue;
            };
            report.checked += 1;
            let Some((latest, url, deprecated)) = parse_thunderstore(&json) else { continue };
            let newer = meta.version.as_deref().map_or(true, |cur| is_newer(&latest, cur));
            if newer || deprecated {
                report.updates.push(ModUpdate { key: meta.key.clone(), source: "thunderstore".into(), current: meta.version.clone(), latest, url, deprecated });
            }
        }
    }
    // A package removed from Thunderstore 404s; only report a real outage.
    if failures > 0 && failures == targets.len() {
        return Err("couldn't reach Thunderstore".into());
    }
    Ok(())
}

async fn check_smapi(http: &reqwest::Client, metas: &[&ModMeta], game_version: Option<&str>, report: &mut UpdateReport) -> Result<(), String> {
    let with_keys: Vec<&ModMeta> = metas.iter().copied().filter(|m| m.source == "smapi" && m.id.is_some() && !m.update_keys.is_empty()).collect();
    if with_keys.is_empty() {
        return Ok(());
    }
    let json = json_of(http.post("https://smapi.io/api/v3.0/mods").json(&smapi_request(&with_keys, game_version)).send().await).await?;
    report.checked += with_keys.len().min(MAX_MODS_PER_SOURCE);
    let suggested = parse_smapi(&json);
    for m in with_keys {
        let Some((latest, url)) = m.id.as_deref().and_then(|id| suggested.get(id)) else { continue };
        if m.version.as_deref().map_or(true, |cur| is_newer(latest, cur)) {
            report.updates.push(ModUpdate { key: m.key.clone(), source: "smapi".into(), current: m.version.clone(), latest: latest.clone(), url: url.clone(), deprecated: false });
        }
    }
    Ok(())
}

/// Check every mod that has a supported source. A failing source is reported
/// in `errors` without failing the others.
pub async fn check(metas: &[ModMeta], base: &str, game_version: Option<&str>) -> Result<UpdateReport, String> {
    let http = client()?;
    let mut report = UpdateReport::default();
    let mr: Vec<&ModMeta> = metas.iter().filter(|m| matches!(m.source.as_str(), "fabric" | "quilt" | "forge")).collect();
    let ts: Vec<&ModMeta> = metas.iter().filter(|m| m.source == "thunderstore").collect();
    let sm: Vec<&ModMeta> = metas.iter().filter(|m| m.source == "smapi").collect();
    if !mr.is_empty() {
        if let Err(e) = check_modrinth(&http, base, &mr, &mut report).await {
            report.errors.push(format!("Modrinth: {e}"));
        }
    }
    if !ts.is_empty() {
        if let Err(e) = check_thunderstore(&http, &ts, &mut report).await {
            report.errors.push(format!("Thunderstore: {e}"));
        }
    }
    if !sm.is_empty() {
        if let Err(e) = check_smapi(&http, &sm, game_version, &mut report).await {
            report.errors.push(format!("SMAPI: {e}"));
        }
    }
    report.updates.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(report)
}

/// Whether any mod in `metas` has a source this module can check.
pub fn checkable(metas: &[ModMeta]) -> bool {
    metas.iter().any(|m| {
        matches!(m.source.as_str(), "fabric" | "quilt" | "forge") && m.is_file
            || thunderstore_target(m).is_some()
            || (m.source == "smapi" && !m.update_keys.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("1.10.0", "1.9.2"));
        assert!(is_newer("2.30.2", "2.30.1"));
        assert!(is_newer("mc1.20.1-0.5.13-fabric", "mc1.20-0.4.10"));
        assert!(is_newer("1.55.0", "1.40.0"));
        assert!(!is_newer("v2.0", "2.0"), "same version, different spelling");
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.2", "1.10"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(is_newer("1.0", "1.0-beta"), "a release beats its pre-release");
        assert!(!is_newer("1.0-beta", "1.0"));
    }

    #[test]
    fn thunderstore_parsing() {
        let meta = ModMeta { source: "thunderstore".into(), id: Some("ValheimModding-Jotunn".into()), ..Default::default() };
        assert_eq!(thunderstore_target(&meta), Some(("ValheimModding".into(), "Jotunn".into())));
        let bad = ModMeta { source: "thunderstore".into(), id: Some("../x-y".into()), ..Default::default() };
        assert_eq!(thunderstore_target(&bad), None, "only plain package names go into the URL");
        let json = serde_json::json!({"package_url":"https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/","is_deprecated":false,"latest":{"version_number":"2.30.2"}});
        assert_eq!(parse_thunderstore(&json), Some(("2.30.2".into(), Some("https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/".into()), false)));
        let evil = serde_json::json!({"package_url":"javascript:alert(1)","latest":{"version_number":"9\u{202e}9"}});
        let (v, url, _) = parse_thunderstore(&evil).unwrap();
        assert_eq!((v.as_str(), url), ("99", None));
    }

    #[test]
    fn smapi_request_and_response() {
        let m = ModMeta { source: "smapi".into(), id: Some("Pathoschild.LookupAnything".into()), version: Some("1.30.0".into()), update_keys: vec!["Nexus:541".into()], ..Default::default() };
        let req = smapi_request(&[&m], Some("1.6.8"));
        assert_eq!(req["mods"][0]["id"], "Pathoschild.LookupAnything");
        assert_eq!(req["mods"][0]["updateKeys"][0], "Nexus:541");
        assert_eq!(req["gameVersion"], "1.6.8");
        let resp = serde_json::json!([
            {"id":"Pathoschild.LookupAnything","suggestedUpdate":{"version":"1.55.0","url":"https://www.nexusmods.com/stardewvalley/mods/541"},"errors":[]},
            {"id":"Other.Mod","suggestedUpdate":null,"errors":[]}
        ]);
        let s = parse_smapi(&resp);
        assert_eq!(s.len(), 1);
        assert_eq!(s["Pathoschild.LookupAnything"].0, "1.55.0");
    }

    #[test]
    fn modrinth_parsing_and_grouping() {
        let json = serde_json::json!({
            "aaa": {"id":"OihdIimA","project_id":"AANobbMI","version_number":"0.5.13","loaders":["quilt","fabric"],"game_versions":["1.20.1"]},
            "bbb": {"id":"X1","project_id":"P2","version_number":"2.0","loaders":["fabric","quilt"],"game_versions":["1.20.1"]},
            "ccc": {"id":"../bad","project_id":"P3","version_number":"1"},
            "ddd": {"id":"Y","project_id":"P4","version_number":"1","loaders":["forge"],"game_versions":["1.19.2"]}
        });
        let v = parse_modrinth(&json);
        assert_eq!(v.len(), 3, "a bad id is dropped");
        assert_eq!(v["aaa"].loaders, vec!["fabric".to_string(), "quilt".to_string()], "sorted, so equal filters group together");
        let groups = modrinth_groups(&v);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|(_, hashes)| hashes.len() == 2));
    }

    #[test]
    fn only_mods_with_a_source_are_checkable() {
        let plain = ModMeta { source: "paradox".into(), ..Default::default() };
        assert!(!checkable(&[plain.clone()]));
        let smapi_no_keys = ModMeta { source: "smapi".into(), id: Some("x".into()), ..Default::default() };
        assert!(!checkable(&[plain, smapi_no_keys]));
        let jar = ModMeta { source: "fabric".into(), is_file: true, ..Default::default() };
        assert!(checkable(&[jar]));
    }

    /// Live check against the real APIs (run by hand: `cargo test live_ -- --ignored`).
    #[tokio::test]
    #[ignore]
    async fn live_thunderstore_and_smapi() {
        let ts = ModMeta { key: "BepInEx/plugins/ValheimModding-Jotunn".into(), source: "thunderstore".into(), id: Some("ValheimModding-Jotunn".into()), version: Some("2.0.0".into()), ..Default::default() };
        let sm = ModMeta { key: "Mods/LookupAnything".into(), source: "smapi".into(), id: Some("Pathoschild.LookupAnything".into()), version: Some("1.30.0".into()), update_keys: vec!["Nexus:541".into()], ..Default::default() };
        let r = check(&[ts, sm], ".", Some("1.6.8")).await.unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.updates.len(), 2, "{:?}", r.updates);
    }
}
