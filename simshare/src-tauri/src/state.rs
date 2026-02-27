use notify::RecommendedWatcher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex as TokioMutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub relative_path: String,
    pub size: u64,
    pub hash: String,
    pub modified: u64,
    pub file_type: FileType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    Mod,
    CustomContent,
    Save,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SimsGame {
    Sims2,
    Sims3,
    Sims4,
}

impl Default for SimsGame {
    fn default() -> Self {
        SimsGame::Sims4
    }
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
    #[serde(default)]
    pub game: SimsGame,
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
}

pub struct AppState {
    pub sims4_path: Option<String>,
    pub local_manifest: FileManifest,
    pub session_type: SessionType,
    pub session_name: String,
    pub local_display_name: String,
    pub session_port: u16,
    pub session_pin: Option<String>,
    pub discovered_peers: Vec<PeerInfo>,
    pub connections: HashMap<String, PeerConnection>,
    #[allow(dead_code)]
    pub file_watcher: Option<RecommendedWatcher>,
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
            sims4_path: None,
            local_manifest: FileManifest::default(),
            session_type: SessionType::None,
            session_name: String::new(),
            local_display_name: String::new(),
            session_port: 9847,
            session_pin: None,
            discovered_peers: Vec::new(),
            connections: HashMap::new(),
            file_watcher: None,
        }
    }
}
