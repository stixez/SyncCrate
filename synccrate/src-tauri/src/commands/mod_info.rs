//! Mod info commands: names, versions, authors and icons read from the
//! metadata files mods ship (`crate::mod_meta`). Read-only.
use crate::mod_meta::{self, IconRef, ModMeta};
use crate::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Icon references from the last metadata pass, so the (large) images are
/// only read when a row actually shows one. Keyed by mod key; cleared when
/// the game folder changes.
static ICONS: std::sync::Mutex<Option<(String, HashMap<String, IconRef>)>> = std::sync::Mutex::new(None);
/// CurseForge logos (`IconRef::Remote`) from the last update check, by mod
/// key: (game folder, icons). Only URLs, never the images; dropped on the
/// next check or when the Content page closes (`forget_curseforge_results`).
static CF_ICONS: std::sync::Mutex<Option<(String, HashMap<String, IconRef>)>> = std::sync::Mutex::new(None);
/// CurseForge logos downloaded since the last check, by URL, as `data:` URLs.
/// One addon spans up to nine folders and a CC set dozens of packages, all
/// with the same logo, and fetching per mod key downloaded it again for each.
/// Memory only, and dropped with the rest of CurseForge's results (their
/// terms allow no keeping). A slot is shared while its download runs, so rows
/// asking at once wait for that one.
static CF_LOGOS: std::sync::Mutex<Option<LogoCache>> = std::sync::Mutex::new(None);
/// Past this much, further logos are still fetched but not kept.
const MAX_CACHED_LOGO_BYTES: usize = 32 * 1024 * 1024;

type LogoSlot = Arc<tokio::sync::OnceCell<Option<String>>>;

#[derive(Default)]
struct LogoCache {
    slots: HashMap<String, LogoSlot>,
    bytes: usize,
}

/// `url`'s logo, from the cache or `fetch` (once, however many ask at once).
async fn cached_logo<F, Fut>(url: &str, fetch: F) -> Option<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let slot: LogoSlot = {
        let mut cache = CF_LOGOS.lock().unwrap_or_else(|e| e.into_inner());
        cache.get_or_insert_with(LogoCache::default).slots.entry(url.to_string()).or_default().clone()
    };
    let mut fetched = false;
    let logo = slot
        .get_or_init(|| {
            fetched = true;
            fetch()
        })
        .await
        .clone();
    if fetched {
        let mut cache = CF_LOGOS.lock().unwrap_or_else(|e| e.into_inner());
        // Only if it's still this slot: results forgotten meanwhile stay forgotten.
        if let Some(cache) = cache.as_mut().filter(|c| c.slots.get(url).is_some_and(|s| Arc::ptr_eq(s, &slot))) {
            match &logo {
                Some(l) if cache.bytes + l.len() <= MAX_CACHED_LOGO_BYTES => cache.bytes += l.len(),
                // A failed download gets another try; one past the cap isn't kept.
                _ => {
                    cache.slots.remove(url);
                }
            }
        }
    }
    logo
}

/// Last metadata result: (game folder, manifest signature, metas).
static METAS: std::sync::Mutex<Option<(String, u64, Vec<ModMeta>)>> = std::sync::Mutex::new(None);

/// Changes when any file is added, removed, resized or touched.
fn manifest_signature(m: &crate::state::FileManifest) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut keys: Vec<(&String, u64, u64)> = m.files.iter().map(|(k, f)| (k, f.size, f.modified)).collect();
    keys.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    keys.hash(&mut h);
    h.finish()
}

/// Metadata for the active game's scanned files. Other games return nothing:
/// only the active game has a current manifest.
#[tauri::command]
pub async fn get_mod_metadata(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<Vec<ModMeta>, String> {
    get_mod_metadata_inner(state.inner(), &game).await
}

pub(crate) async fn get_mod_metadata_inner(state: &Arc<Mutex<AppState>>, game: &str) -> Result<Vec<ModMeta>, String> {
    let (base, files, signature) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Ok(Vec::new());
        }
        let base = s.active_game_path()?;
        let signature = manifest_signature(&s.local_manifest);
        (base, s.local_manifest.files.keys().cloned().collect::<Vec<_>>(), signature)
    };
    // The Content page asks after every rescan; with an unchanged file list
    // the answer is the same, and probing every folder (and opening every
    // jar) again was the bulk of the work on big mod folders.
    if let Some(cached) = METAS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|c| c.0 == base && c.1 == signature) {
        return Ok(cached.2.clone());
    }
    let b = base.clone();
    let metas = tokio::task::spawn_blocking(move || mod_meta::extract(&b, &files)).await.map_err(|e| e.to_string())?;
    *METAS.lock().unwrap_or_else(|e| e.into_inner()) = Some((base.clone(), signature, metas.clone()));
    let icons = metas.iter().filter(|m| m.has_icon).map(|m| (m.key.clone(), m.icon.clone())).collect();
    *ICONS.lock().unwrap_or_else(|e| e.into_inner()) = Some((base, icons));
    Ok(metas)
}

/// The mod's icon as a `data:` URL, or None (not an image, too big, gone).
#[tauri::command]
pub async fn get_mod_icon(key: String) -> Result<Option<String>, String> {
    let found = ICONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|(base, icons)| icons.get(&key).map(|i| (base.clone(), i.clone())));
    let Some((base, icon)) = found else {
        // No picture of its own: maybe CurseForge has one.
        let remote = CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|(_, icons)| icons.get(&key).cloned());
        return Ok(match remote {
            Some(IconRef::Remote(url)) => cached_logo(&url, || crate::curseforge::logo_data_url(&url)).await,
            _ => None,
        });
    };
    tokio::task::spawn_blocking(move || mod_meta::icon_data_url(&base, &key, &icon)).await.map_err(|e| e.to_string())
}

/// The Content page closed: CurseForge's terms don't allow keeping their
/// data around, so the logo links and downloaded logos go with it.
#[tauri::command]
pub fn forget_curseforge_results() {
    *CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *CF_LOGOS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Ask Modrinth / Thunderstore / SMAPI / CurseForge which of the active
/// game's mods have newer versions (`crate::mod_updates`). Only runs when the
/// user clicks: it sends mod ids, jar hashes and file fingerprints to those
/// services (CurseForge through synccrate.app).
#[tauri::command]
pub async fn check_mod_updates(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<crate::mod_updates::UpdateReport, String> {
    let (base, files, game_version, mods_folder, curseforge) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Err("Open this game's Content page first.".into());
        }
        let gv = s.game_info.get(&game).and_then(|g| g.game_version.clone());
        let def = s.game_registry.games.iter().find(|g| g.id == game);
        let mods_folder = def.and_then(|d| d.content_types.iter().find(|ct| ct.id == "mods" || ct.id == "addons").map(|ct| ct.rel_folder()));
        let curseforge = def.and_then(|d| d.curseforge.clone());
        (s.active_game_path()?, s.local_manifest.files.keys().cloned().collect::<Vec<_>>(), gv, mods_folder, curseforge)
    };
    forget_curseforge_results();
    let cf_files = match (&curseforge, mods_folder) {
        (Some(cf), Some(folder)) => crate::curseforge::select_files(cf, &folder, &files),
        _ => Vec::new(),
    };
    let b = base.clone();
    let metas = tokio::task::spawn_blocking(move || mod_meta::extract(&b, &files)).await.map_err(|e| e.to_string())?;
    if !crate::mod_updates::checkable(&metas, cf_files.len()) {
        return Ok(crate::mod_updates::UpdateReport::default());
    }
    let report = crate::mod_updates::check(&metas, &base, game_version.as_deref(), curseforge.as_ref(), &cf_files).await?;
    let icons: HashMap<String, IconRef> =
        report.metas.iter().filter(|m| matches!(m.icon, IconRef::Remote(_))).map(|m| (m.key.clone(), m.icon.clone())).collect();
    *CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()) = Some((base, icons));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn each_logo_url_is_fetched_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch = |answer: Option<&'static str>| {
            let calls = calls.clone();
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                answer.map(str::to_string)
            }
        };
        let url = "https://media.forgecdn.net/avatars/test-once.png";
        // Folders of one addon asking at the same time share one download.
        let one = || cached_logo(url, fetch(Some("data:image/png;base64,AA==")));
        let got = tokio::join!(one(), one(), one(), one());
        assert!([got.0, got.1, got.2, got.3].iter().all(|g| g.as_deref() == Some("data:image/png;base64,AA==")));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cached_logo(url, fetch(None)).await.is_some(), "kept for later rows");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // A failed download isn't remembered.
        let bad = "https://media.forgecdn.net/avatars/test-fails.png";
        assert_eq!(cached_logo(bad, fetch(None)).await, None);
        assert!(cached_logo(bad, fetch(Some("data:image/png;base64,BB=="))).await.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
