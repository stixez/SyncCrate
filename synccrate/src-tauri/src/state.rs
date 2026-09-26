use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::registry::GameRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub relative_path: String,
    pub size: u64,
    pub hash: String,
    pub modified: u64,
    pub file_type: String,
}

/// Dynamic folder permissions: keys are content type IDs from the game registry.
pub type SyncFolderPermissions = HashMap<String, bool>;

/// Check whether a file is allowed by the current folder permissions.
/// The `content_type_id` is looked up in the permissions map.
/// If not found, defaults to true (allow).
pub fn is_file_allowed(perms: &SyncFolderPermissions, content_type_id: &str) -> bool {
    perms.get(content_type_id).copied().unwrap_or(true)
}

/// Map a file_type string to its content type ID using the game registry.
/// Falls back to allowing the file if no mapping is found.
pub fn file_type_to_content_id(
    file_type: &str,
    content_types: &[crate::registry::ContentType],
) -> Option<String> {
    content_types.iter()
        .find(|ct| {
            ct.file_type == file_type
                || ct.classify_by_extension.values().any(|v| v == file_type)
        })
        .map(|ct| ct.id.clone())
}

/// Map a file to its content type ID using both its relative path and file_type.
///
/// Several content types of one game can share a file_type (e.g. "mods",
/// "scenarios" and "blueprints" are all "CustomContent"), so the file_type
/// alone is ambiguous. Prefer the content type whose folder contains the file
/// (longest folder wins); fall back to the file_type-only mapping.
pub fn content_id_for_file(
    relative_path: &str,
    file_type: &str,
    content_types: &[crate::registry::ContentType],
) -> Option<String> {
    let rel = relative_path.replace('\\', "/");
    let rel_lower = rel.to_lowercase();
    let mut best: Option<(&crate::registry::ContentType, usize)> = None;
    for ct in content_types {
        let type_matches = ct.file_type == file_type
            || ct.classify_by_extension.values().any(|v| v == file_type);
        if !type_matches {
            continue;
        }
        let folder = ct.folder.replace('\\', "/");
        let folder = folder.trim_end_matches('/');
        let depth = if folder.is_empty() || folder == "." {
            0
        } else if rel_lower.starts_with(&format!("{}/", folder.to_lowercase())) {
            folder.len()
        } else {
            continue;
        };
        if best.map_or(true, |(_, d)| depth > d) {
            best = Some((ct, depth));
        }
    }
    best.map(|(ct, _)| ct.id.clone())
        .or_else(|| file_type_to_content_id(file_type, content_types))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileManifest {
    pub files: HashMap<String, FileInfo>,
    pub generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub mod_count: usize,
    pub version: String,
    pub pin_required: bool,
    #[serde(default)]
    pub game_info: Option<GameInfo>,
    /// Game the host is sharing, from discovery (None for older hosts / peers).
    #[serde(default)]
    pub game_id: Option<String>,
    /// Every address the peer was discovered on, best candidate first.
    /// `ip` is always `addresses[0]` when non-empty; connecting tries each in turn.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// The host's iroh endpoint id (hex) from discovery, so crew members can
    /// be recognised on the LAN. None for hosts older than 0.6.0.
    #[serde(default)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_type: SessionType,
    pub name: String,
    pub port: u16,
    pub peer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionType {
    Host,
    Client,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_type: SessionType,
    pub name: String,
    pub port: u16,
    pub peers: Vec<PeerInfo>,
    pub is_syncing: bool,
    pub pin: Option<String>,
    #[serde(default)]
    pub host_ips: Vec<String>,
    /// Whether LAN discovery (mDNS / UDP broadcast) is running for this host session.
    #[serde(default)]
    pub discovery_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncAction {
    SendToRemote(FileInfo),
    ReceiveFromRemote(FileInfo),
    Conflict {
        local: FileInfo,
        remote: FileInfo,
    },
    Delete(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub total_bytes: u64,
    #[serde(default)]
    pub excluded: Vec<String>,
    #[serde(default)]
    pub resumed_files: u64,
    /// "Keep both" conflict resolutions: maps the local path the remote copy
    /// should be saved under (e.g. "Mods/x_remote.package") to the path the
    /// file actually has on the remote peer ("Mods/x.package"). The peer only
    /// serves files by their real path, so the rename happens locally.
    #[serde(default)]
    pub keep_both: HashMap<String, String>,
    /// Plan hash computed when the plan was built (before conflict resolution
    /// and resume filtering). Used as the stable identity for resume checkpoints.
    #[serde(default)]
    pub plan_hash: Option<String>,
    /// Game and folder the plan was computed against. `execute_sync` refuses a
    /// plan whose target no longer matches the active game/path (real bug: a
    /// plan computed for one folder executed against a newly picked folder).
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub base_path: String,
    /// "Use theirs" resolutions, keyed by the remote path of the queued
    /// `ReceiveFromRemote`. The download replaces `local_path` (which can differ
    /// from the remote path in case or `.disabled` state) only if the local file
    /// still has `local_hash`, i.e. it wasn't changed after the compare.
    #[serde(default)]
    pub use_theirs: HashMap<String, ReplaceTarget>,
    /// Conflicts the user already resolved, keyed by local path, so a
    /// resolution can be changed later (e.g. KeepBoth -> UseTheirs).
    #[serde(default)]
    pub resolved_conflicts: HashMap<String, ConflictPair>,
    /// Host files dropped because no content type of the active game accepts
    /// their path (e.g. an older host sharing a different game).
    #[serde(default)]
    pub skipped_foreign: usize,
    /// Game the host said it shares (`None` for hosts older than 0.5.6).
    #[serde(default)]
    pub host_game: Option<String>,
    /// Host files the client has only as a disabled copy with the same content.
    #[serde(default)]
    pub disabled_locally: usize,
    /// Host files the host has disabled while the client has them enabled.
    #[serde(default)]
    pub disabled_on_host: usize,
    /// Plan-level warning for the UI (e.g. "host may be sharing a different game").
    #[serde(default)]
    pub warning: Option<String>,
    /// Informational line (e.g. "3 host files can't be saved here and were
    /// skipped"). Unlike `warning`, it doesn't stop Stay in sync: one
    /// `desktop.ini` on the host used to switch auto-pull off for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Pack files (by relative path) the connected host doesn't have with the
    /// pack's exact hash. Only set on a plan from `compute_pack_plan`.
    #[serde(default)]
    pub pack_unavailable: Vec<String>,
    /// Built by stay-in-sync (`commands::stay_in_sync`): its downloads extend
    /// the existing undo record instead of replacing it.
    #[serde(default)]
    pub auto_pull: bool,
}

/// One file in a modpack's manifest. No content — packs are shareable
/// because they stay tiny regardless of how much they describe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackFile {
    pub relative_path: String,
    pub size: u64,
    pub hash: String,
}

/// A host's join info embedded in a pack, so "get missing files" doesn't
/// need a separate discovery/join-code step. Best-effort: only present when
/// the pack was exported while its author was hosting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackJoin {
    pub code: String,
}

/// A shareable snapshot of "my exact setup" for one game: which files, not
/// their bytes. `format_version` is bumped only on a breaking change to this
/// shape; unknown extra fields from a newer app already round-trip fine
/// through serde without it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModPack {
    pub format_version: u32,
    pub app_version: String,
    pub game_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub created_at: u64,
    /// Content type ids actually represented in `files` (informational; the
    /// importer's own registry entry, not the exporter's, decides folders).
    #[serde(default)]
    pub content_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<PackJoin>,
    pub files: Vec<PackFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplaceTarget {
    pub local_path: String,
    pub local_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictPair {
    pub local: FileInfo,
    pub remote: FileInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PackType {
    ExpansionPack,
    GamePack,
    StuffPack,
    Kit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PackId {
    pub code: String,
    pub pack_type: PackType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub id: PackId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameInfo {
    pub game_version: Option<String>,
    pub installed_packs: Vec<PackInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompatibilityStatus {
    Compatible,
    MissingPacks,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModCompatibility {
    pub mod_path: String,
    pub required_packs: Vec<PackId>,
    pub missing_packs: Vec<PackId>,
    pub status: CompatibilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Resolution {
    KeepMine,
    UseTheirs,
    KeepBoth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub author: String,
    pub created_at: u64,
    pub mods: Vec<ProfileMod>,
    #[serde(default = "default_game")]
    pub game: String,
}

fn default_game() -> String {
    "sims4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMod {
    pub relative_path: String,
    pub hash: String,
    pub size: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileComparison {
    pub profile_name: String,
    pub matched: usize,
    pub missing: Vec<String>,
    pub modified: Vec<String>,
    pub extra: Vec<String>,
}

pub struct PeerConnection {
    pub info: PeerInfo,
    pub stream: Arc<TokioMutex<crate::network::stream::PeerStream>>,
    pub remote_manifest: Option<FileManifest>,
    pub sync_plan: Option<SyncPlan>,
    pub is_syncing: bool,
}

pub struct AppState {
    pub game_paths: HashMap<String, String>,
    pub active_game: String,
    pub local_manifest: FileManifest,
    pub game_info: HashMap<String, GameInfo>,
    pub session_type: SessionType,
    pub session_name: String,
    pub local_display_name: String,
    pub pending_client_peer_id: Option<String>,
    pub session_port: u16,
    pub session_pin: Option<String>,
    pub folder_permissions: SyncFolderPermissions,
    pub discovered_peers: Vec<PeerInfo>,
    pub connections: HashMap<String, PeerConnection>,
    #[allow(dead_code)]
    pub file_watcher: Option<crate::watcher::file_watcher::FolderWatcher>,
    /// Game registry loaded at startup (immutable after init).
    pub game_registry: GameRegistry,
    /// Game IDs the user has added to their library.
    pub user_library: Vec<String>,
    /// Library games hidden from the sidebar (still configured).
    pub hidden_games: Vec<String>,
    /// The user's crews (`crate::crews`), loaded at startup.
    pub crews: crate::crews::CrewStore,
    /// Where `crews` is saved; None in tests and when the file on disk
    /// couldn't be read (so a newer app's data is never overwritten).
    pub crews_path: Option<std::path::PathBuf>,
    /// Hex of this install's iroh endpoint id (sent in Hello/Welcome).
    pub local_node_id: Option<String>,
    /// This session's chat (`crate::chat`); cleared when a new session starts.
    pub chat: crate::chat::ChatLog,
    /// Wrong-PIN attempts per source while hosting (`network::pin_guard`).
    pub pin_guard: crate::network::pin_guard::PinGuard,
    /// Host side: file offers from connected friends, by peer id (`crate::offers`).
    pub offers_in: HashMap<String, crate::offers::IncomingOffer>,
    /// Client side: our offer to the host, and whether the host takes offers.
    pub offer_out: Option<crate::offers::OutgoingOffer>,
    pub offers_available: bool,
    /// Bumped whenever a session ends; see `transfer::still_hosting`.
    pub host_epoch: u64,
}

impl AppState {
    /// Get a list of all connected peers' info.
    pub fn peers(&self) -> Vec<PeerInfo> {
        self.connections.values().map(|c| c.info.clone()).collect()
    }

    /// Check if any peer is currently syncing.
    pub fn is_any_syncing(&self) -> bool {
        self.connections.values().any(|c| c.is_syncing)
    }

    /// Check whether a file is allowed by the current folder permissions.
    ///
    /// `folder_permissions` is keyed by content type ID (e.g. "saves", "mods"),
    /// NOT by file_type ("Save", "CustomContent"), so the file must be mapped
    /// through the active game's content types first (using its relative path
    /// to disambiguate content types that share a file_type). Passing
    /// `info.file_type` straight to `is_file_allowed` never matches a key and
    /// silently allows everything.
    pub fn is_file_info_allowed(&self, info: &FileInfo) -> bool {
        if self.folder_permissions.is_empty() {
            return true;
        }
        match content_id_for_file(&info.relative_path, &info.file_type, self.active_content_types()) {
            Some(id) => is_file_allowed(&self.folder_permissions, &id),
            None => true,
        }
    }

    fn active_content_types(&self) -> &[crate::registry::ContentType] {
        self.game_registry
            .games
            .iter()
            .find(|g| g.id == self.active_game)
            .map(|g| g.content_types.as_slice())
            .unwrap_or(&[])
    }

    /// Get the path for the active game, or error if not configured.
    pub fn active_game_path(&self) -> Result<String, String> {
        self.game_paths
            .get(&self.active_game)
            .cloned()
            .ok_or_else(|| {
                let label = self.game_label(&self.active_game);
                format!("{} path not set. Please set it first.", label)
            })
    }

    /// Get a human-readable label for a game ID.
    pub fn game_label(&self, game_id: &str) -> String {
        crate::registry::build_registry_map(&self.game_registry)
            .get(game_id)
            .map(|d| d.label.clone())
            .unwrap_or_else(|| game_id.to_string())
    }

    /// Resolve an optional peer_id: if None and we're a client with one connection, auto-resolve.
    pub fn resolve_peer_id(&self, peer_id: Option<String>) -> Result<String, String> {
        match peer_id {
            Some(id) => {
                if self.connections.contains_key(&id) {
                    Ok(id)
                } else {
                    Err(format!("Peer '{}' not found", id))
                }
            }
            None => {
                if self.connections.len() == 1 {
                    Ok(self.connections.keys().next().unwrap().clone())
                } else if self.connections.is_empty() {
                    Err("No active connections".to_string())
                } else {
                    Err("Multiple peers connected — specify a peer_id".to_string())
                }
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            game_paths: HashMap::new(),
            active_game: "sims4".to_string(),
            local_manifest: FileManifest::default(),
            game_info: HashMap::new(),
            session_type: SessionType::None,
            session_name: String::new(),
            local_display_name: String::new(),
            pending_client_peer_id: None,
            session_port: 9847,
            session_pin: None,
            folder_permissions: HashMap::new(),
            discovered_peers: Vec::new(),
            connections: HashMap::new(),
            file_watcher: None,
            game_registry: GameRegistry { version: 0, games: Vec::new() },
            user_library: Vec::new(),
            hidden_games: Vec::new(),
            crews: crate::crews::CrewStore::default(),
            crews_path: None,
            local_node_id: None,
            chat: crate::chat::ChatLog::default(),
            pin_guard: Default::default(),
            offers_in: HashMap::new(),
            offer_out: None,
            offers_available: false,
            host_epoch: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ContentType;

    fn ct(id: &str, folder: &str, file_type: &str, classify: &[(&str, &str)]) -> ContentType {
        ContentType {
            id: id.to_string(),
            label: id.to_string(),
            folder: folder.to_string(),
            extensions: Vec::new(),
            file_type: file_type.to_string(),
            classify_by_extension: classify
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            icon: String::new(),
            color: String::new(),
            syncable: true,
            recursive: true,
            must_contain: None,
            exclude_files: Vec::new(),
        }
    }

    fn file(path: &str, file_type: &str) -> FileInfo {
        FileInfo {
            relative_path: path.to_string(),
            size: 1,
            hash: "h".to_string(),
            modified: 0,
            file_type: file_type.to_string(),
        }
    }

    #[test]
    fn content_id_uses_folder_to_disambiguate_shared_file_type() {
        let cts = vec![
            ct("mods", "mods", "CustomContent", &[]),
            ct("scenarios", "scenarios", "CustomContent", &[]),
            ct("blueprints", ".", "CustomContent", &[]),
        ];
        assert_eq!(content_id_for_file("mods/a.zip", "CustomContent", &cts).as_deref(), Some("mods"));
        assert_eq!(content_id_for_file("scenarios/s.zip", "CustomContent", &cts).as_deref(), Some("scenarios"));
        assert_eq!(content_id_for_file("bp.dat", "CustomContent", &cts).as_deref(), Some("blueprints"));
    }

    #[test]
    fn content_id_handles_classified_extensions() {
        let cts = vec![
            ct("mods", "Mods", "CustomContent", &[("ts4script", "Mod")]),
            ct("saves", "Saves", "Save", &[]),
        ];
        assert_eq!(content_id_for_file("Mods/x.ts4script", "Mod", &cts).as_deref(), Some("mods"));
        assert_eq!(content_id_for_file("Saves/slot.save", "Save", &cts).as_deref(), Some("saves"));
    }

    #[test]
    fn folder_permissions_are_matched_by_content_id_not_file_type() {
        let mut state = AppState::default();
        state.active_game = "g".to_string();
        state.game_registry.games = Vec::new();
        let def: crate::registry::GameDefinition = {
            let mut def = crate::registry::load_registry()
                .games
                .into_iter()
                .find(|g| g.id == "sims4")
                .expect("sims4 in registry");
            def.id = "g".to_string();
            def.content_types = vec![
                ct("mods", "Mods", "CustomContent", &[]),
                ct("saves", "Saves", "Save", &[]),
            ];
            def
        };
        state.game_registry.games.push(def);
        state.folder_permissions.insert("saves".to_string(), false);
        state.folder_permissions.insert("mods".to_string(), true);

        assert!(!state.is_file_info_allowed(&file("Saves/slot.save", "Save")));
        assert!(state.is_file_info_allowed(&file("Mods/a.package", "CustomContent")));
        // The old (buggy) lookup keyed by file_type allowed everything:
        assert!(is_file_allowed(&state.folder_permissions, "Save"));
    }
}
