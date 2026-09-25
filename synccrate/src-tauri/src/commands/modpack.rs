//! Shareable modpacks: "my exact setup" for one game as a small manifest
//! file (`.scpack`) — game id, content types, and a file list (relative
//! path, size, sha256), no bytes. Importing shows have/missing/different
//! against the local folder; "get missing files" turns the pack into an
//! ordinary `SyncPlan` (via `sync::diff::compute_pack_plan`) restricted to
//! the pack's own paths, so it runs through the real `execute_sync`/backup/
//! undo pipeline unchanged.
//!
//! Deliberately its own type rather than an extension of `ModProfile`
//! (`commands/profiles.rs`): profiles are unversioned, cover only the
//! game's first content type, and are already a shipped, tested feature.
//! Packs need a format version, an arbitrary set of content types, and
//! optional join info, none of which belongs bolted onto profiles. The
//! reusable parts — hashed scanning, path validation, the JSON-envelope
//! export convention — are duplicated in miniature rather than shared,
//! to avoid touching that already-shipped module for this feature.
use crate::commands::files::{get_game_def, missing_folder_error, resolve_game};
use crate::state::{AppState, FileManifest, ModPack, PackFile, PackJoin};
use crate::sync::diff::match_key;
use base64::{engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD}, Engine as _};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const FORMAT_VERSION: u32 = 1;
/// Matches `MAX_BACKUP_FILES` (`commands::backup`): a sane ceiling on any
/// file-list-shaped input from disk, not a real-world pack size.
const MAX_PACK_FILES: usize = 100_000;
/// Encoded-size cap for the text/deep-link export. A `.scpack` file has no
/// such limit; this only decides whether offering a pasteable link is
/// worthwhile (a few dozen files fits a chat message, thousands don't).
pub(crate) const MAX_LINK_BYTES: usize = 8 * 1024;
const LINK_PREFIX: &str = "synccrate://pack/";
/// A 100k-file pack is ~15 MB of JSON; anything far past that isn't a pack.
const MAX_PACK_FILE_BYTES: u64 = 64 * 1024 * 1024;

fn is_valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Same shape as `profiles::validate_profile_paths`: reject anything but
/// plain relative segments. Duplicated rather than shared (see module doc).
fn is_plain_relative_path(path: &str) -> bool {
    let p = std::path::Path::new(path);
    !path.is_empty()
        && !path.contains(':')
        && !path.starts_with(['/', '\\'])
        && p.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

/// Treat a pack as untrusted input: reject a future format we don't
/// understand, an unknown game, path traversal/absolute paths, a malformed
/// hash, or an unreasonably large file list, before anything else touches it.
pub(crate) fn validate_pack(pack: &ModPack) -> Result<(), String> {
    if pack.format_version == 0 || pack.format_version > FORMAT_VERSION {
        return Err(format!(
            "This pack needs a newer version of SyncCrate (format {}, this app supports up to {}).",
            pack.format_version, FORMAT_VERSION
        ));
    }
    if pack.game_id.trim().is_empty() {
        return Err("Pack has no game.".to_string());
    }
    if pack.files.len() > MAX_PACK_FILES {
        return Err(format!("Pack has too many files (>{MAX_PACK_FILES}).", ));
    }
    for f in &pack.files {
        if !is_plain_relative_path(&f.relative_path) {
            return Err(format!("Invalid file path in pack: {}", f.relative_path));
        }
        if !is_valid_hash(&f.hash) {
            return Err(format!("Invalid hash in pack for {}", f.relative_path));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn create_pack(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
    content_types: Option<Vec<String>>,
    paths: Option<Vec<String>>,
    name: String,
    description: String,
    include_join_code: bool,
) -> Result<ModPack, String> {
    create_pack_inner(state.inner(), game, content_types, paths, name, description, include_join_code).await
}

pub(crate) async fn create_pack_inner(
    state: &Arc<Mutex<AppState>>,
    game: Option<String>,
    content_types: Option<Vec<String>>,
    paths: Option<Vec<String>>,
    name: String,
    description: String,
    include_join_code: bool,
) -> Result<ModPack, String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return Err("Pack name must be 1-128 characters".to_string());
    }
    if description.len() > 1024 {
        return Err("Description must be under 1024 characters".to_string());
    }

    let target_game = {
        let app_state = state.lock().await;
        match game {
            Some(ref g) => require_active_or_configured(&app_state, g)?,
            None => app_state.active_game.clone(),
        }
    };
    let manifest = crate::commands::files::scan_files_inner(state, Some(target_game.clone()), true).await?;

    let app_state = state.lock().await;
    let cts = get_game_def(&app_state.game_registry, &target_game)
        .map(|d| d.content_types.clone())
        .unwrap_or_default();
    let wanted_paths: Option<std::collections::HashSet<String>> =
        paths.map(|p| p.iter().map(|s| match_key(s)).collect());

    let mut files = Vec::new();
    let mut included_types = std::collections::HashSet::new();
    for f in manifest.files.values() {
        if let Some(ref wanted) = wanted_paths {
            if !wanted.contains(&match_key(&f.relative_path)) {
                continue;
            }
        } else if let Some(ref selected) = content_types {
            let id = crate::state::content_id_for_file(&f.relative_path, &f.file_type, &cts);
            if !id.as_ref().is_some_and(|id| selected.contains(id)) {
                continue;
            }
        }
        if let Some(id) = crate::state::content_id_for_file(&f.relative_path, &f.file_type, &cts) {
            included_types.insert(id);
        }
        files.push(PackFile { relative_path: f.relative_path.clone(), size: f.size, hash: f.hash.clone() });
    }
    if files.is_empty() {
        return Err("Nothing matches that selection — pick at least one file or content type.".to_string());
    }
    if files.len() > MAX_PACK_FILES {
        return Err(format!("Too many files for a pack (>{MAX_PACK_FILES}). Narrow the selection."));
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let is_host = app_state.session_type == crate::state::SessionType::Host;
    drop(app_state);
    let join = if include_join_code && is_host {
        crate::commands::session::join_code_for(state).await.ok().map(|code| PackJoin { code })
    } else {
        None
    };

    Ok(ModPack {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        game_id: target_game,
        name,
        description,
        author: String::new(),
        created_at: crate::utils::timestamp_now(),
        content_types: included_types.into_iter().collect(),
        join,
        files,
    })
}

/// Like `AppState::require_active`, but a saved-but-not-currently-loaded
/// path is fine too — exporting a pack shouldn't require switching games.
fn require_active_or_configured(app_state: &AppState, game: &str) -> Result<String, String> {
    let id = resolve_game(app_state, game)?;
    if !app_state.game_paths.contains_key(&id) {
        let label = app_state.game_label(&id);
        return Err(format!("{} path not set. Please set it first.", label));
    }
    Ok(id)
}

#[tauri::command]
pub async fn save_pack(pack: ModPack, dest: String) -> Result<(), String> {
    validate_pack(&pack)?;
    let dest = if dest.to_lowercase().ends_with(".scpack") { dest } else { format!("{}.scpack", dest) };
    let data = serde_json::to_string_pretty(&pack).map_err(|e| e.to_string())?;
    std::fs::write(&dest, data).map_err(|e| e.to_string())
}

/// Base64 of the pack's compact JSON, for pasting or a `synccrate://pack/`
/// deep link. Refused above `MAX_LINK_BYTES` — file export has no such cap.
#[tauri::command]
pub async fn pack_to_link(pack: ModPack) -> Result<String, String> {
    validate_pack(&pack)?;
    let data = serde_json::to_string(&pack).map_err(|e| e.to_string())?;
    // URL-safe and unpadded: standard base64's `+`, `/` and `=` got
    // percent-encoded or cut off by browsers and chat apps, and with this
    // alphabet trailing punctuation (Discord's `)` or `.`) can be stripped
    // without ambiguity. `decode_pack_payload` still reads the old alphabet.
    let encoded = URL_SAFE_NO_PAD.encode(data.as_bytes());
    if encoded.len() > MAX_LINK_BYTES {
        return Err(format!(
            "This pack ({} files) is too big for a text link ({} KB, max {} KB) — share the .scpack file instead.",
            pack.files.len(),
            encoded.len() / 1024,
            MAX_LINK_BYTES / 1024
        ));
    }
    Ok(format!("{LINK_PREFIX}{encoded}"))
}

#[tauri::command]
pub async fn load_pack_file(path: String) -> Result<ModPack, String> {
    read_pack_file(std::path::Path::new(&path))
}

/// Shared with `commands::open_intent` (double-clicked `.scpack` files), so
/// the file is size-capped before it is read: it may come from anywhere.
pub(crate) fn read_pack_file(path: &std::path::Path) -> Result<ModPack, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_PACK_FILE_BYTES {
        return Err("That file is too big to be a SyncCrate pack.".to_string());
    }
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let pack: ModPack = serde_json::from_str(&data).map_err(|e| format!("Not a valid pack: {}", e))?;
    validate_pack(&pack)?;
    Ok(pack)
}

#[tauri::command]
pub async fn load_pack_link(text: String) -> Result<ModPack, String> {
    // Same cleanup as a clicked link (scheme case, percent-encoding, trailing
    // chat punctuation); a bare payload without the prefix is accepted too.
    let trimmed = text.trim();
    match crate::commands::open_intent::classify(trimmed, |_| true) {
        Some(crate::commands::open_intent::OpenTarget::PackLink(payload)) => decode_pack_payload(&payload),
        Some(crate::commands::open_intent::OpenTarget::Invalid(reason)) => Err(reason),
        _ => decode_pack_payload(trimmed),
    }
}

/// The base64 part of a pack link. Accepts the current URL-safe alphabet and
/// the standard one that links from before deep-link support used.
pub(crate) fn decode_pack_payload(encoded: &str) -> Result<ModPack, String> {
    let encoded = encoded.trim().trim_end_matches('=');
    if encoded.len() > MAX_LINK_BYTES * 2 {
        return Err("That pack link is too long.".to_string());
    }
    let engine = if encoded.contains(['+', '/']) { &STANDARD_NO_PAD } else { &URL_SAFE_NO_PAD };
    let bytes = engine.decode(encoded).map_err(|_| "Not a valid pack link".to_string())?;
    let pack: ModPack = serde_json::from_slice(&bytes).map_err(|e| format!("Not a valid pack: {}", e))?;
    validate_pack(&pack)?;
    Ok(pack)
}

#[derive(Debug, Clone, Serialize)]
pub struct PackFileStatus {
    pub relative_path: String,
    pub size: u64,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PackComparison {
    pub pack_name: String,
    pub pack_game: String,
    /// Set instead of erroring, so the UI can offer the same "switch to
    /// `<game>`" prompt the wrong-game connection flow uses — the pack
    /// itself is still valid, just for a different game than is active.
    pub wrong_game: bool,
    pub have: usize,
    pub have_bytes: u64,
    pub missing: Vec<PackFileStatus>,
    pub different: Vec<PackFileStatus>,
}

/// Have/missing/different against the local folder — no host involved yet.
/// Case- and `.disabled`-aware via `match_key`, same as a real sync compare.
pub(crate) fn compare_pack_to_local(pack: &ModPack, local: &FileManifest, cts: &[crate::registry::ContentType]) -> PackComparison {
    let mut local_by_key: std::collections::HashMap<String, Vec<&crate::state::FileInfo>> = std::collections::HashMap::new();
    for info in local.files.values() {
        local_by_key.entry(match_key(&info.relative_path)).or_default().push(info);
    }

    let mut have = 0usize;
    let mut have_bytes = 0u64;
    let mut missing = Vec::new();
    let mut different = Vec::new();
    for pf in &pack.files {
        let key = match_key(&pf.relative_path);
        let content_type = crate::sync::diff::content_type_for(cts, &pf.relative_path).map(|(ct, _)| ct.id.clone());
        match local_by_key.get(&key) {
            Some(cands) if cands.iter().any(|l| !pf.hash.is_empty() && l.hash == pf.hash) => {
                have += 1;
                have_bytes += pf.size;
            }
            Some(_) => different.push(PackFileStatus { relative_path: pf.relative_path.clone(), size: pf.size, content_type }),
            None => missing.push(PackFileStatus { relative_path: pf.relative_path.clone(), size: pf.size, content_type }),
        }
    }
    PackComparison {
        pack_name: pack.name.clone(),
        pack_game: pack.game_id.clone(),
        wrong_game: false,
        have,
        have_bytes,
        missing,
        different,
    }
}

#[tauri::command]
pub async fn compare_pack(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    pack: ModPack,
) -> Result<PackComparison, String> {
    compare_pack_inner(state.inner(), pack).await
}

pub(crate) async fn compare_pack_inner(state: &Arc<Mutex<AppState>>, pack: ModPack) -> Result<PackComparison, String> {
    validate_pack(&pack)?;
    let active_game = state.lock().await.active_game.clone();
    // An unknown game id (e.g. a very old legacy name) can't be resolved at
    // all; treat that the same as "not this game" rather than erroring.
    let pack_game_resolved = {
        let app_state = state.lock().await;
        resolve_game(&app_state, &pack.game_id).unwrap_or_else(|_| pack.game_id.clone())
    };
    if pack_game_resolved != active_game {
        return Ok(PackComparison { pack_name: pack.name, pack_game: pack.game_id, wrong_game: true, ..Default::default() });
    }

    let manifest = crate::commands::files::scan_files_inner(state, Some(active_game.clone()), true).await?;
    let app_state = state.lock().await;
    let cts = get_game_def(&app_state.game_registry, &active_game).map(|d| d.content_types.clone()).unwrap_or_default();
    Ok(compare_pack_to_local(&pack, &manifest, &cts))
}

/// Filter the pack down to what the connected host can actually serve (via
/// `diff::compute_pack_plan`) and store the result as the peer's sync plan,
/// exactly like `compute_sync_plan` — `execute_sync`/`resolve_conflict`/the
/// presync backup all then run completely unchanged.
#[tauri::command]
pub async fn compute_pack_sync_plan(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    peer_id: Option<String>,
    pack: ModPack,
) -> Result<crate::state::SyncPlan, String> {
    compute_pack_sync_plan_inner(state.inner(), peer_id, pack).await
}

pub(crate) async fn compute_pack_sync_plan_inner(
    state: &Arc<Mutex<AppState>>,
    peer_id: Option<String>,
    pack: ModPack,
) -> Result<crate::state::SyncPlan, String> {
    validate_pack(&pack)?;

    let needs_rehash = {
        let app_state = state.lock().await;
        if app_state.is_any_syncing() {
            return Err("A sync is already running — wait for it to finish".to_string());
        }
        let files = &app_state.local_manifest.files;
        files.is_empty() || files.values().any(|f| f.hash.is_empty())
    };
    if needs_rehash {
        crate::commands::files::scan_files_inner(state, None, true).await?;
    }

    let mut app_state = state.lock().await;
    if app_state.is_any_syncing() {
        return Err("A sync is already running — wait for it to finish".to_string());
    }
    let resolved_id = app_state.resolve_peer_id(peer_id)?;
    let active_game = app_state.active_game.clone();
    if pack.game_id != active_game && resolve_game(&app_state, &pack.game_id).ok().as_deref() != Some(active_game.as_str()) {
        return Err(format!("This pack is for a different game than {}.", app_state.game_label(&active_game)));
    }
    let base_path = app_state.active_game_path()?;
    if !std::path::Path::new(&base_path).is_dir() {
        let label = app_state.game_label(&active_game);
        return Err(missing_folder_error(&label, &base_path));
    }

    let conn = app_state.connections.get(&resolved_id).ok_or("Peer not found")?;
    let remote = conn.remote_manifest.clone().ok_or("No remote manifest available. Connect to a peer first.")?;

    let (mut plan, unavailable) = crate::sync::diff::compute_pack_plan(&app_state.local_manifest, &remote, &pack.files);
    plan.game_id = active_game;
    plan.base_path = base_path;
    plan.pack_unavailable = unavailable;
    plan.plan_hash = Some(crate::sync::diff::compute_plan_hash(&plan));

    let conn = app_state.connections.get_mut(&resolved_id).ok_or("Peer disconnected")?;
    conn.sync_plan = Some(plan.clone());
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(files: &[(&str, &str)]) -> ModPack {
        ModPack {
            format_version: FORMAT_VERSION,
            app_version: "0.5.6".into(),
            game_id: "sims4".into(),
            name: "Test pack".into(),
            description: String::new(),
            author: String::new(),
            created_at: 0,
            content_types: vec![],
            join: None,
            files: files.iter().map(|(p, h)| PackFile { relative_path: p.to_string(), size: 1, hash: h.to_string() }).collect(),
        }
    }

    const H: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn round_trips_through_json() {
        let p = pack(&[("Mods/a.package", H)]);
        let json = serde_json::to_string(&p).unwrap();
        let back: ModPack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.game_id, "sims4");
        assert_eq!(back.files.len(), 1);
        assert_eq!(back.files[0].hash, H);
    }

    #[test]
    fn unknown_extra_fields_from_a_newer_app_are_ignored() {
        let json = serde_json::json!({
            "format_version": 1, "app_version": "9.9.9", "game_id": "sims4", "name": "n",
            "created_at": 0, "files": [], "some_future_field": {"nested": true},
        });
        let p: ModPack = serde_json::from_value(json).unwrap();
        assert_eq!(p.game_id, "sims4");
    }

    #[test]
    fn validate_rejects_future_format_versions() {
        let mut p = pack(&[]);
        p.format_version = FORMAT_VERSION + 1;
        assert!(validate_pack(&p).is_err());
        p.format_version = 0;
        assert!(validate_pack(&p).is_err());
        p.format_version = FORMAT_VERSION;
        assert!(validate_pack(&p).is_ok());
    }

    #[test]
    fn validate_rejects_traversal_and_absolute_paths() {
        for bad in ["../x", "Mods/../../x", r"\Windows\x", "/etc/x", "C:x", r"C:\x", "Mods/a.package:ads", ""] {
            let p = pack(&[(bad, H)]);
            assert!(validate_pack(&p).is_err(), "{bad}");
        }
        assert!(validate_pack(&pack(&[("Mods/a.package", H)])).is_ok());
    }

    #[test]
    fn validate_rejects_malformed_hashes() {
        for bad in ["", "not-hex", "aaaa", &"a".repeat(63), &"g".repeat(64)] {
            assert!(validate_pack(&pack(&[("Mods/a.package", bad)])).is_err(), "{bad}");
        }
    }

    #[test]
    fn validate_rejects_empty_game_id() {
        let mut p = pack(&[("Mods/a.package", H)]);
        p.game_id = String::new();
        assert!(validate_pack(&p).is_err());
    }

    #[test]
    fn validate_rejects_oversized_file_lists() {
        let files: Vec<(String, String)> =
            (0..MAX_PACK_FILES + 1).map(|i| (format!("Mods/{i}.package"), H.to_string())).collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, h)| (p.as_str(), h.as_str())).collect();
        assert!(validate_pack(&pack(&refs)).is_err());
    }

    fn manifest(entries: &[(&str, &str)]) -> FileManifest {
        let mut m = FileManifest::default();
        for (p, h) in entries {
            m.files.insert(
                p.to_string(),
                crate::state::FileInfo { relative_path: p.to_string(), size: 1, hash: h.to_string(), modified: 0, file_type: "Mod".into() },
            );
        }
        m
    }

    #[test]
    fn compare_reports_have_missing_and_different() {
        let p = pack(&[("Mods/have.package", H), ("Mods/missing.package", H), ("Mods/different.package", H)]);
        let local = manifest(&[("Mods/have.package", H), ("Mods/different.package", "b".repeat(64).as_str())]);
        let cmp = compare_pack_to_local(&p, &local, &[]);
        assert_eq!(cmp.have, 1);
        assert_eq!(cmp.missing.len(), 1);
        assert_eq!(cmp.missing[0].relative_path, "Mods/missing.package");
        assert_eq!(cmp.different.len(), 1);
        assert_eq!(cmp.different[0].relative_path, "Mods/different.package");
    }

    #[test]
    fn compare_matches_case_only_and_disabled_names() {
        let p = pack(&[("Mods/CC/Hair.package", H)]);
        // Locally disabled, different case: still counts as "have".
        let local = manifest(&[("Mods/cc/hair.package.disabled", H)]);
        let cmp = compare_pack_to_local(&p, &local, &[]);
        assert_eq!(cmp.have, 1);
        assert!(cmp.missing.is_empty() && cmp.different.is_empty());
    }
}
