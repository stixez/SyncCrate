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
            Some(IconRef::Remote(url)) => crate::curseforge::logo_data_url(&url).await,
            _ => None,
        });
    };
    tokio::task::spawn_blocking(move || mod_meta::icon_data_url(&base, &key, &icon)).await.map_err(|e| e.to_string())
}

/// The Content page closed: CurseForge's terms don't allow keeping their
/// data around, so the logo links go with it.
#[tauri::command]
pub fn forget_curseforge_results() {
    *CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Ask Modrinth / Thunderstore / SMAPI / CurseForge which of the active
/// game's mods have newer versions (`crate::mod_updates`). Only runs when the
/// user clicks: it sends mod ids, jar hashes and file fingerprints to those
/// services (CurseForge through synccrate.app).
#[tauri::command]
pub async fn check_mod_updates(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<crate::mod_updates::UpdateReport, String> {
    let (base, files, game_version, mods_folder) = {
        let s = state.lock().await;
        if s.active_game != game {
            return Err("Open this game's Content page first.".into());
        }
        let gv = s.game_info.get(&game).and_then(|g| g.game_version.clone());
        let mods_folder = crate::registry::build_registry_map(&s.game_registry)
            .get(&game)
            .and_then(|d| d.content_types.iter().find(|ct| ct.id == "mods").map(|ct| ct.rel_folder()));
        (s.active_game_path()?, s.local_manifest.files.keys().cloned().collect::<Vec<_>>(), gv, mods_folder)
    };
    *CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let cf_files = mods_folder.map(|f| crate::curseforge::select_files(&game, &f, &files)).unwrap_or_default();
    let b = base.clone();
    let metas = tokio::task::spawn_blocking(move || mod_meta::extract(&b, &files)).await.map_err(|e| e.to_string())?;
    if !crate::mod_updates::checkable(&metas, cf_files.len()) {
        return Ok(crate::mod_updates::UpdateReport::default());
    }
    let report = crate::mod_updates::check(&metas, &base, game_version.as_deref(), &game, &cf_files).await?;
    let icons: HashMap<String, IconRef> =
        report.metas.iter().filter(|m| matches!(m.icon, IconRef::Remote(_))).map(|m| (m.key.clone(), m.icon.clone())).collect();
    *CF_ICONS.lock().unwrap_or_else(|e| e.into_inner()) = Some((base, icons));
    Ok(report)
}

