//! Game artwork: Steam store art fetched at runtime and cached on disk, plus
//! user-chosen custom covers.
//!
//! Art is never bundled with the app (it belongs to the publishers). Like
//! other launchers, we download the official Steam library assets on first
//! use and keep them in `<config>/synccrate/art/`, so they work offline after
//! that. Games that aren't on Steam (WoW, Minecraft, ...) fall back to the
//! app's generated tile unless the user picks a custom cover.
//!
//! Images are returned as `data:` URLs because the webview CSP only allows
//! `'self'` and `data:` images, and it avoids enabling the asset protocol.

use crate::state::AppState;
use base64::Engine;
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
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
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

fn data_url(path: &Path) -> Option<String> {
    let mime = mime_for(path)?;
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Download one Steam asset into `dest`, trying each CDN. Writes to a temp
/// file first so a half-finished download never looks like a cached image.
async fn download(app_id: u32, file: &str, dest: &Path) -> Result<(), String> {
    let _permit = DOWNLOADS.acquire().await.map_err(|e| e.to_string())?;
    if dest.is_file() {
        return Ok(()); // another request finished it while we waited
    }
    // reqwest is built without a bundled crypto provider (shared with iroh and
    // the updater); install ring once, the same way the updater plugin does.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("SyncCrate/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::from("no CDN reachable");
    for cdn in CDNS {
        let url = format!("{cdn}/{app_id}/{file}");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
                // Guard against error pages served with 200.
                if bytes.len() < 1024 {
                    last_err = format!("{url}: response too small");
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

    let app_id = {
        let st = state.lock().await;
        st.game_registry.games.iter().find(|g| g.id == game_id).and_then(|g| g.steam_app_id)
    };
    let Some(app_id) = app_id else { return Ok(None) };

    let dest = dir.join(format!("{app_id}_{kind}.jpg"));
    if !dest.is_file() {
        if let Err(e) = download(app_id, file, &dest).await {
            log::debug!("Game art for {game_id} ({kind}) unavailable: {e}");
            return Ok(None);
        }
    }
    Ok(data_url(&dest))
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
    fn custom_cover_found_and_removed() {
        let dir = std::env::temp_dir().join(format!("synccrate_art_{}", std::process::id()));
        let custom = dir.join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        assert!(custom_cover(&dir, "g1").is_none());
        std::fs::write(custom.join("g1.png"), b"img").unwrap();
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
