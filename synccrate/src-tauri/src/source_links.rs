//! "Share as a link": the host names a file or folder of mods whose creator
//! forbids re-uploads (Patreon early-access CC, "no reupload" rules,
//! CurseForge's terms). Those files never leave the host. Its manifest lists
//! them separately (`LinkedFile`, with the creator's page), friends see "Get
//! these from the creator" in their plan, and a `FileRequest` for one is
//! refused even when a client asks for it directly.
//!
//! Links are stored per game at `<config>/synccrate/source_links/<game>.json`
//! and matched like sync matches paths (`diff::match_key`: case-insensitive,
//! `.disabled` and legacy `_Disabled/` seen through), so a link survives the
//! host disabling the mod or a folder renamed only in case.
use crate::state::FileManifest;
use crate::sync::diff::match_key;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// The refusal a client gets for a linked file, whatever it asked for.
pub const REFUSAL: &str = "The host shares this mod as a link to its creator.";
const MAX_LABEL: usize = 100;
const MAX_PREFIX: usize = 1024;
/// Far more than anyone links by hand; keeps the file (and every manifest
/// check against it) small.
pub const MAX_LINKS: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceLink {
    /// Game-folder-relative path ('/'-separated): one file, or a folder that
    /// covers every file under it.
    pub prefix: String,
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Host -> client in `ManifestResponse`: a file the host has but only shares
/// as a link. Size and hash let the friend see whether their copy matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkedFile {
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkStatus {
    /// The friend doesn't have the file.
    Missing,
    /// The friend has another version of it.
    Different,
}

/// A row of the plan's "Get these from the creator" list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceLinkItem {
    pub path: String,
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
    pub status: LinkStatus,
}

/// A link URL as typed, or why it can't be used. Same rules as a mod's own
/// website (`mod_meta::clean_url`): it opens in the friend's browser on one
/// click, and it comes from another PC.
pub fn validate_url(raw: &str) -> Result<String, String> {
    crate::mod_meta::clean_url(raw)
        .filter(|u| u.len() > "https://".len())
        .ok_or_else(|| "Use a full https:// link (up to 300 characters, no spaces).".to_string())
}

/// A prefix as stored: '/'-separated, no empty or `.` segments, inside the
/// game folder.
pub fn normalize_prefix(raw: &str) -> Result<String, String> {
    let bad = || "That path isn't inside the game folder.".to_string();
    let joined = raw
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect::<Vec<_>>()
        .join("/");
    // `C:x`/`C:/x` is a drive path on Windows and validate_relative only
    // rejects ':' there, so check it here for every platform.
    if joined.is_empty() || joined.len() > MAX_PREFIX || joined.contains(':') || raw.starts_with(['/', '\\']) {
        return Err(bad());
    }
    if joined.split('/').any(|s| s == "..") {
        return Err(bad());
    }
    crate::utils::validate_relative(&joined).map_err(|_| bad())?;
    Ok(joined)
}

/// Short, single-line text a friend sees next to the link.
pub fn clean_label(raw: Option<&str>) -> Option<String> {
    let s: String = raw?
        .chars()
        .filter(|c| !c.is_control() && !crate::chat::is_bidi_control(*c))
        .take(MAX_LABEL)
        .collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn covers_key(prefix_key: &str, path_key: &str) -> bool {
    !prefix_key.is_empty()
        && path_key.strip_prefix(prefix_key).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The most specific link covering `path` (a file link beats its folder's).
pub fn link_for<'a>(links: &'a [SourceLink], path: &str) -> Option<&'a SourceLink> {
    let key = match_key(path);
    links
        .iter()
        .map(|l| (match_key(&l.prefix), l))
        .filter(|(k, _)| covers_key(k, &key))
        .max_by_key(|(k, _)| k.len())
        .map(|(_, l)| l)
}

/// Host side: move every file a link covers out of the manifest a friend
/// gets, and describe it as a `LinkedFile` instead (sorted by path).
pub fn split_linked(manifest: &mut FileManifest, links: &[SourceLink]) -> Vec<LinkedFile> {
    if links.is_empty() {
        return Vec::new();
    }
    let keyed: Vec<(String, &SourceLink)> = links.iter().map(|l| (match_key(&l.prefix), l)).collect();
    let mut out = Vec::new();
    manifest.files.retain(|path, info| {
        let key = match_key(path);
        let Some((_, link)) = keyed.iter().filter(|(k, _)| covers_key(k, &key)).max_by_key(|(k, _)| k.len()) else {
            return true;
        };
        out.push(LinkedFile { path: path.clone(), size: info.size, hash: info.hash.clone(), url: link.url.clone(), label: link.label.clone() });
        false
    });
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Insert or replace (by prefix, matched like paths) a link in `links`.
pub fn upsert(links: &mut Vec<SourceLink>, link: SourceLink) -> Result<(), String> {
    let key = match_key(&link.prefix);
    if let Some(existing) = links.iter_mut().find(|l| match_key(&l.prefix) == key) {
        *existing = link;
        return Ok(());
    }
    if links.len() >= MAX_LINKS {
        return Err(format!("You can share up to {MAX_LINKS} links per game."));
    }
    links.push(link);
    links.sort_by(|a, b| a.prefix.to_lowercase().cmp(&b.prefix.to_lowercase()));
    Ok(())
}

// ---------------------------------------------------------------------------
// Storage

/// Per game, as last read or saved. The host checks links on every
/// `FileRequest`; reading the file each time would be thousands of reads in
/// one big sync.
static CACHE: LazyLock<RwLock<HashMap<String, Vec<SourceLink>>>> = LazyLock::new(Default::default);

fn links_path(game: &str) -> std::path::PathBuf {
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    crate::utils::config_root()
        .join("synccrate")
        .join("source_links")
        .join(format!("{}.json", if safe.is_empty() { "unknown" } else { &safe }))
}

/// The game's links. `Err` only when the file exists but couldn't be read:
/// the host then refuses to serve rather than share files it was told not to.
pub fn load(game: &str) -> Result<Vec<SourceLink>, String> {
    if let Some(links) = CACHE.read().unwrap_or_else(|e| e.into_inner()).get(game) {
        return Ok(links.clone());
    }
    let stored: Vec<SourceLink> = crate::utils::read_json_strict(&links_path(game))?.unwrap_or_default();
    // Hand-edited or from a newer version: keep only what the rules allow.
    let mut links = Vec::new();
    for l in stored {
        if let (Ok(prefix), Ok(url)) = (normalize_prefix(&l.prefix), validate_url(&l.url)) {
            let _ = upsert(&mut links, SourceLink { prefix, url, label: clean_label(l.label.as_deref()) });
        }
    }
    CACHE.write().unwrap_or_else(|e| e.into_inner()).insert(game.to_string(), links.clone());
    Ok(links)
}

pub fn save(game: &str, links: &[SourceLink]) -> Result<(), String> {
    let path = links_path(game);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save the links: {e}"))?;
    }
    crate::utils::write_json_atomic(&path, &links)?;
    CACHE.write().unwrap_or_else(|e| e.into_inner()).insert(game.to_string(), links.to_vec());
    Ok(())
}

pub fn set_link(game: &str, prefix: &str, url: &str, label: Option<&str>) -> Result<Vec<SourceLink>, String> {
    let link = SourceLink { prefix: normalize_prefix(prefix)?, url: validate_url(url)?, label: clean_label(label) };
    let mut links = load(game)?;
    upsert(&mut links, link)?;
    save(game, &links)?;
    Ok(links)
}

pub fn remove_link(game: &str, prefix: &str) -> Result<Vec<SourceLink>, String> {
    let key = match_key(&normalize_prefix(prefix)?);
    let mut links = load(game)?;
    let before = links.len();
    links.retain(|l| match_key(&l.prefix) != key);
    if links.len() != before {
        save(game, &links)?;
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FileInfo;

    fn covers(prefix: &str, path: &str) -> bool {
        covers_key(&match_key(prefix), &match_key(path))
    }

    fn link(prefix: &str, url: &str) -> SourceLink {
        SourceLink { prefix: prefix.into(), url: url.into(), label: None }
    }

    #[test]
    fn a_file_link_covers_only_that_file_and_its_disabled_twin() {
        assert!(covers("Mods/CC/hair.package", "Mods/CC/hair.package"));
        assert!(covers("Mods/CC/hair.package", "mods/cc/HAIR.package"), "case-insensitive like sync");
        assert!(covers("Mods/CC/hair.package", "Mods/CC/hair.package.disabled"));
        assert!(covers("Mods/CC/hair.package", "Mods/_Disabled/CC/hair.package"), "legacy disabled folder");
        assert!(!covers("Mods/CC/hair.package", "Mods/CC/hair.package2"));
        assert!(!covers("Mods/CC/hair.package", "Mods/CC/hair"));
    }

    #[test]
    fn a_folder_link_covers_everything_under_it_and_nothing_beside_it() {
        assert!(covers("Mods/Creator", "Mods/Creator/a.package"));
        assert!(covers("Mods/Creator", "mods/creator/sub/b.package.disabled"));
        assert!(!covers("Mods/Creator", "Mods/CreatorTwo/a.package"), "a prefix must end at a folder");
        assert!(!covers("Mods/Creator", "Mods/a.package"));
        assert!(!covers("", "Mods/a.package"));
    }

    #[test]
    fn the_most_specific_link_wins() {
        let links = vec![link("Mods/Creator", "https://a.example/folder"), link("Mods/Creator/x.package", "https://a.example/file")];
        assert_eq!(link_for(&links, "Mods/Creator/x.package").unwrap().url, "https://a.example/file");
        assert_eq!(link_for(&links, "Mods/Creator/y.package").unwrap().url, "https://a.example/folder");
        assert!(link_for(&links, "Mods/other.package").is_none());
    }

    #[test]
    fn urls_must_be_plain_https() {
        assert_eq!(validate_url("  https://patreon.com/creator ").unwrap(), "https://patreon.com/creator");
        assert!(validate_url("HTTPS://example.com/x").is_ok());
        assert!(validate_url("http://example.com").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("https://").is_err());
        assert!(validate_url("https://exa mple.com").is_err());
        assert!(validate_url("https://example.com/\u{7}").is_err());
        assert!(validate_url(&format!("https://example.com/{}", "a".repeat(300))).is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn prefixes_must_stay_inside_the_game_folder() {
        assert_eq!(normalize_prefix("Mods\\CC\\hair.package").unwrap(), "Mods/CC/hair.package");
        assert_eq!(normalize_prefix("./Mods//Creator/").unwrap(), "Mods/Creator");
        assert_eq!(normalize_prefix("@saves/World1").unwrap(), "@saves/World1");
        for bad in ["", "/", ".", "../x", "Mods/../../x", "/etc/passwd", "\\Windows\\x", "C:/x", "C:x", "Mods/a:b", "Mods/CON"] {
            assert!(normalize_prefix(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn labels_are_short_single_lines() {
        assert_eq!(clean_label(Some("  Hair by Anna \n")).as_deref(), Some("Hair by Anna"));
        assert_eq!(clean_label(Some("   ")), None);
        assert_eq!(clean_label(None), None);
        assert_eq!(clean_label(Some(&"x".repeat(500))).unwrap().len(), MAX_LABEL);
    }

    #[test]
    fn upsert_replaces_by_prefix_ignoring_case() {
        let mut links = vec![link("Mods/Creator", "https://a.example/1")];
        upsert(&mut links, link("mods/creator", "https://a.example/2")).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://a.example/2");
        upsert(&mut links, link("Mods/Other", "https://a.example/3")).unwrap();
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn the_host_manifest_moves_linked_files_out() {
        let info = |p: &str, h: &str| FileInfo { relative_path: p.into(), size: 7, hash: h.into(), modified: 0, file_type: "CustomContent".into() };
        let mut m = FileManifest::default();
        for (p, h) in [("Mods/free.package", "f"), ("Mods/Creator/a.package", "a"), ("Mods/Creator/b.package.disabled", "b"), ("Mods/solo.package", "s")] {
            m.files.insert(p.into(), info(p, h));
        }
        let links = vec![
            SourceLink { prefix: "Mods/creator".into(), url: "https://c.example".into(), label: Some("Creator".into()) },
            link("Mods/SOLO.package", "https://s.example"),
        ];
        let linked = split_linked(&mut m, &links);
        assert_eq!(m.files.keys().collect::<Vec<_>>(), vec!["Mods/free.package"]);
        let paths: Vec<&str> = linked.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["Mods/Creator/a.package", "Mods/Creator/b.package.disabled", "Mods/solo.package"]);
        assert_eq!(linked[0].label.as_deref(), Some("Creator"));
        assert_eq!((linked[0].size, linked[0].hash.as_str()), (7, "a"));
        assert_eq!(linked[2].url, "https://s.example");
        // No links: the manifest is untouched.
        let mut m2 = m.clone();
        assert!(split_linked(&mut m2, &[]).is_empty());
        assert_eq!(m2.files.len(), 1);
    }

    #[tokio::test]
    async fn links_round_trip_through_storage() {
        let _g = crate::testutil::e2e_guard().await;
        let game = "links-test-game";
        assert!(load(game).unwrap().is_empty());
        set_link(game, "Mods\\Creator", "https://c.example", Some("Creator")).unwrap();
        set_link(game, "Mods/solo.package", "https://s.example", None).unwrap();
        assert!(set_link(game, "../x", "https://s.example", None).is_err());
        assert!(set_link(game, "Mods/x", "http://s.example", None).is_err());
        // From disk, not the cache.
        CACHE.write().unwrap().remove(game);
        let links = load(game).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].prefix, "Mods/Creator");
        let links = remove_link(game, "mods/creator").unwrap();
        assert_eq!(links.len(), 1);
        save(game, &[]).unwrap();
    }
}
