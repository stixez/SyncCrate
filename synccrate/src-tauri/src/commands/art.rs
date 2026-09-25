//! Game artwork: Steam store art fetched at runtime and cached on disk, plus
//! user-chosen custom covers.
//!
//! Art is never bundled with the app (it belongs to the publishers). Like
//! other launchers, we download the official Steam library assets on first
//! use and keep them in `<config>/synccrate/art/`, so they work offline after
//! that. Games that aren't on Steam (WoW, Minecraft, ...) use `art_urls` from
//! the registry instead: official key art hosted by the publisher (Blizzard,
//! the Microsoft Store, the developer's site). If a link dies, the UI falls
//! back to its generated tile. A user's custom cover beats both.
//!
//! Images are returned as `data:` URLs because the webview CSP only allows
//! `'self'` and `data:` images, and it avoids enabling the asset protocol.

use crate::state::AppState;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

/// The browser grid can ask for ~100 covers at once; don't open 100 connections.
static DOWNLOADS: Semaphore = Semaphore::const_new(6);

const CDNS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps",
    "https://cdn.cloudflare.steamstatic.com/steam/apps",
];

/// Largest custom cover we accept (they're stored and base64'd on every load).
const MAX_CUSTOM_BYTES: u64 = 8 * 1024 * 1024;

fn art_dir() -> PathBuf {
    // config_root, like every other data path, so tests can redirect it.
    let config = crate::utils::config_root();
    let dir = config.join("synccrate").join("art");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Steam file name for an art kind: `cover` (600x900 portrait), `header`
/// (460x215 banner) or `hero` (wide background).
fn steam_file(kind: &str) -> Option<&'static str> {
    match kind {
        "cover" => Some("library_600x900.jpg"),
        "header" => Some("header.jpg"),
        "hero" => Some("library_hero.jpg"),
        _ => None,
    }
}

fn mime_for(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Game ids come from the frontend; keep them to safe file-name characters.
fn safe_id(game_id: &str) -> Result<&str, String> {
    if !game_id.is_empty()
        && game_id.len() <= 64
        && game_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(game_id)
    } else {
        Err("Invalid game id".into())
    }
}

fn custom_cover(dir: &Path, game_id: &str) -> Option<PathBuf> {
    ["jpg", "jpeg", "png", "webp"]
        .iter()
        .map(|ext| dir.join("custom").join(format!("{game_id}.{ext}")))
        .find(|p| p.is_file())
}

/// Image type from the file's magic bytes. Publisher CDNs don't always match
/// the extension (Blizzard serves JPEGs named `.png`), so never trust it.
/// Box art is a few hundred KB; stop reading well before a bad response
/// could exhaust memory.
const MAX_ART_BYTES: usize = 10 * 1024 * 1024;

async fn read_capped(mut resp: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err("image too large".into());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if out.len() + chunk.len() > max {
            return Err("image too large".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn data_url(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mime = sniff_mime(&bytes)?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Download the first of `urls` that returns a real image into `dest`. Writes
/// to a temp file first so a half-finished download never looks cached.
async fn download(urls: &[String], dest: &Path) -> Result<(), String> {
    let _permit = DOWNLOADS.acquire().await.map_err(|e| e.to_string())?;
    if dest.is_file() {
        return Ok(()); // another request finished it while we waited
    }
    // (read_capped / MAX_ART_BYTES below bound the download.)
    // reqwest is built without a bundled crypto provider (shared with iroh and
    // the updater); install ring once, the same way the updater plugin does.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("SyncCrate/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        // https only, a few hops: art URLs come from the registry, but a CDN
        // redirect must not downgrade to http or bounce around.
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() == "https" && a.previous().len() < 5 { a.follow() } else { a.stop() }
        }))
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::from("no source reachable");
    for url in urls {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = match read_capped(resp, MAX_ART_BYTES).await {
                    Ok(b) => b,
                    Err(e) => {
                        last_err = format!("{url}: {e}");
                        continue;
                    }
                };
                // Guard against error pages / placeholders served with 200.
                if bytes.len() < 1024 || sniff_mime(&bytes).is_none() {
                    last_err = format!("{url}: not an image");
                    continue;
                }
                let tmp = dest.with_extension("part");
                std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Ok(resp) => last_err = format!("{url}: HTTP {}", resp.status()),
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    Err(last_err)
}

/// Artwork for a game as a `data:` URL, or `None` when there is none (not on
/// Steam and no custom cover, or offline with nothing cached). A custom cover
/// replaces every kind.
#[tauri::command]
pub async fn get_game_art(
    game_id: String,
    kind: String,
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<String>, String> {
    let game_id = safe_id(&game_id)?.to_string();
    let file = steam_file(&kind).ok_or("Unknown art kind")?;
    let dir = art_dir();

    if let Some(custom) = custom_cover(&dir, &game_id) {
        return Ok(data_url(&custom));
    }

    let (app_id, art_urls) = {
        let st = state.lock().await;
        match st.game_registry.games.iter().find(|g| g.id == game_id) {
            Some(g) => (g.steam_app_id, g.art_urls.clone()),
            None => return Ok(None),
        }
    };

    let (urls, dest) = if let Some(app_id) = app_id {
        let urls: Vec<String> = CDNS.iter().map(|cdn| format!("{cdn}/{app_id}/{file}")).collect();
        (urls, dir.join(format!("{app_id}_{kind}.jpg")))
    } else if let Some(url) = publisher_url(&art_urls, &kind) {
        // Keyed by URL, so a registry update with a new link fetches fresh art.
        let key = hex::encode(&Sha256::digest(url.as_bytes())[..8]);
        (vec![url], dir.join(format!("web_{key}.img")))
    } else {
        return Ok(None);
    };

    if !dest.is_file() {
        if let Err(e) = download(&urls, &dest).await {
            log::debug!("Game art for {game_id} ({kind}) unavailable: {e}");
            return Ok(None);
        }
    }
    Ok(data_url(&dest))
}

/// The registry link for an art kind, falling back to the other kinds (most
/// publishers only have one good wide image). HTTPS only.
fn publisher_url(art_urls: &HashMap<String, String>, kind: &str) -> Option<String> {
    [kind, "hero", "header", "cover"]
        .iter()
        .filter_map(|k| art_urls.get(*k))
        .find(|u| u.starts_with("https://"))
        .cloned()
}

/// Use a local image as the game's cover (any game, including non-Steam ones).
#[tauri::command]
pub async fn set_custom_game_art(game_id: String, source_path: String) -> Result<String, String> {
    let game_id = safe_id(&game_id)?;
    let src = PathBuf::from(&source_path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| mime_for(Path::new(&format!("x.{e}"))).is_some())
        .ok_or("Pick a JPG, PNG or WebP image")?;
    let size = std::fs::metadata(&src).map_err(|e| e.to_string())?.len();
    if size > MAX_CUSTOM_BYTES {
        return Err("That image is too large (max 8 MB)".into());
    }

    let custom_dir = art_dir().join("custom");
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;
    remove_custom(&custom_dir, game_id);
    let dest = custom_dir.join(format!("{game_id}.{ext}"));
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    data_url(&dest).ok_or_else(|| "Couldn't read the copied image".into())
}

/// Remove a custom cover, going back to Steam art (or the generated tile).
#[tauri::command]
pub async fn clear_custom_game_art(game_id: String) -> Result<(), String> {
    let game_id = safe_id(&game_id)?;
    remove_custom(&art_dir().join("custom"), game_id);
    Ok(())
}

/// Game ids that currently have a custom cover (for the Settings "Reset" button).
#[tauri::command]
pub async fn list_custom_game_art() -> Result<Vec<String>, String> {
    let Ok(entries) = std::fs::read_dir(art_dir().join("custom")) else { return Ok(Vec::new()) };
    let mut ids: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| mime_for(p).is_some())
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn remove_custom(custom_dir: &Path, game_id: &str) {
    for ext in ["jpg", "jpeg", "png", "webp"] {
        let _ = std::fs::remove_file(custom_dir.join(format!("{game_id}.{ext}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_id_rejects_path_tricks() {
        assert!(safe_id("sims4-reshade").is_ok());
        assert!(safe_id("wow_retail").is_ok());
        assert!(safe_id("../evil").is_err());
        assert!(safe_id("a/b").is_err());
        assert!(safe_id("").is_err());
    }

    #[test]
    fn steam_file_maps_kinds() {
        assert_eq!(steam_file("cover"), Some("library_600x900.jpg"));
        assert_eq!(steam_file("hero"), Some("library_hero.jpg"));
        assert_eq!(steam_file("logo"), None);
    }

    #[test]
    fn sniff_mime_ignores_extension() {
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A]), Some("image/png"));
        assert_eq!(sniff_mime(b"RIFF0000WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_mime(b"<!DOCTYPE html>"), None);
    }

    #[test]
    fn publisher_url_falls_back_across_kinds() {
        let mut m = HashMap::new();
        m.insert("hero".to_string(), "https://example.com/hero.jpg".to_string());
        assert_eq!(publisher_url(&m, "cover").as_deref(), Some("https://example.com/hero.jpg"));
        m.insert("cover".to_string(), "https://example.com/cover.jpg".to_string());
        assert_eq!(publisher_url(&m, "cover").as_deref(), Some("https://example.com/cover.jpg"));
        m.insert("header".to_string(), "http://insecure.example.com/x.jpg".to_string());
        assert_eq!(publisher_url(&m, "header").as_deref(), Some("https://example.com/hero.jpg"));
        assert_eq!(publisher_url(&HashMap::new(), "hero"), None);
    }

    #[test]
    fn every_game_has_art_source() {
        // Steam id or publisher links: no game should be left with only the generated tile.
        for g in crate::registry::load_registry().games {
            assert!(
                g.steam_app_id.is_some() || publisher_url(&g.art_urls, "hero").is_some(),
                "{} has no art source",
                g.id
            );
        }
    }

    #[test]
    fn custom_cover_found_and_removed() {
        let dir = std::env::temp_dir().join(format!("synccrate_art_{}", std::process::id()));
        let custom = dir.join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        assert!(custom_cover(&dir, "g1").is_none());
        std::fs::write(custom.join("g1.png"), [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00]).unwrap();
        assert_eq!(custom_cover(&dir, "g1"), Some(custom.join("g1.png")));
        assert!(data_url(&custom.join("g1.png")).unwrap().starts_with("data:image/png;base64,"));
        remove_custom(&custom, "g1");
        assert!(custom_cover(&dir, "g1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn most_registry_games_have_steam_ids() {
        let reg = crate::registry::load_registry();
        let with_id = reg.games.iter().filter(|g| g.steam_app_id.is_some()).count();
        assert!(with_id >= 90, "expected Steam ids for most games, got {with_id}");
    }
}
