use notify::RecommendedWatcher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub total_bytes: u64,
    #[serde(default)]
    pub excluded: Vec<String>,
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
    pub stream: Arc<TokioMutex<TcpStream>>,
    pub remote_manifest: Option<FileManifest>,
    pub sync_plan: Option<SyncPlan>,
    pub is_syncing: bool,
    pub supports_compression: bool,
}

pub struct AppState {
    pub game_paths: HashMap<String, String>,
    pub active_game: String,
    pub local_manifest: FileManifest,
    pub game_info: HashMap<String, GameInfo>,
    pub session_type: SessionType,
    pub session_name: String,
    pub local_display_name: String,
    pub session_port: u16,
    pub session_pin: Option<String>,
    pub folder_permissions: SyncFolderPermissions,
    pub discovered_peers: Vec<PeerInfo>,
    pub connections: HashMap<String, PeerConnection>,
    #[allow(dead_code)]
    pub file_watcher: Option<RecommendedWatcher>,
    /// Game registry loaded at startup (immutable after init).
    pub game_registry: GameRegistry,
    /// Game IDs the user has added to their library.
    pub user_library: Vec<String>,
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
            session_port: 9847,
            session_pin: None,
            folder_permissions: HashMap::new(),
            discovered_peers: Vec::new(),
            connections: HashMap::new(),
            file_watcher: None,
            game_registry: GameRegistry { version: 0, games: Vec::new() },
            user_library: Vec::new(),
        }
    }
}
