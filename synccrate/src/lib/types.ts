// Game and FileType are now dynamic strings driven by the game registry.
// Game IDs like "sims4", "wow_retail", "minecraft_java" come from game_registry.json.
// FileType values like "Mod", "CustomContent", "Save", "Addon" come from content_types.
export type Game = string;
export type FileType = string;

/** @deprecated Use Game (string) instead */
export type SimsGame = Game;

export interface FileInfo {
  relative_path: string;
  size: number;
  hash: string;
  modified: number;
  file_type: FileType;
}

export interface FileManifest {
  files: Record<string, FileInfo>;
  generated_at: number;
}

export type PackType = "ExpansionPack" | "GamePack" | "StuffPack" | "Kit";

export interface PackId {
  code: string;
  pack_type: PackType;
}

export interface PackInfo {
  id: PackId;
  name: string;
}

export interface GameInfo {
  game_version: string | null;
  installed_packs: PackInfo[];
}

export type CompatibilityStatus = "Compatible" | "MissingPacks" | "Unknown";

export interface ModCompatibility {
  mod_path: string;
  required_packs: PackId[];
  missing_packs: PackId[];
  status: CompatibilityStatus;
}

export interface PeerInfo {
  id: string;
  name: string;
  ip: string;
  port: number;
  mod_count: number;
  version: string;
  pin_required: boolean;
  game_info?: GameInfo | null;
  /** Every address the host was discovered on, best first (`ip` is the first). */
  addresses?: string[];
}

export interface SessionInfo {
  session_type: "Host" | "Client" | "None";
  name: string;
  port: number;
  peer_count: number;
}

export interface SessionStatus {
  session_type: "Host" | "Client" | "None";
  name: string;
  port: number;
  peers: PeerInfo[];
  is_syncing: boolean;
  pin: string | null;
  host_ips: string[];
  /** Host only: whether LAN discovery (mDNS / UDP broadcast) is advertising this session. */
  discovery_active?: boolean;
}

export interface FirewallStatus {
  /** False on macOS/Linux — nothing to manage. */
  supported: boolean;
  /** Whether the rule lookup succeeded. */
  checked: boolean;
  has_allow_rule: boolean;
  has_block_rule: boolean;
  exe_path: string;
}

export interface LocalInterface {
  name: string;
  ip: string;
  is_virtual: boolean;
  is_primary: boolean;
}

export interface NetworkDiagnostics {
  interfaces: LocalInterface[];
  firewall: FirewallStatus;
  session_port: number;
  discovery_port: number;
  discovery_active: boolean;
  is_elevated: boolean;
}

export interface ConnectionTestResult {
  reachable: boolean;
  message: string;
  latency_ms: number | null;
}

export interface SyncAction {
  SendToRemote?: FileInfo;
  ReceiveFromRemote?: FileInfo;
  Conflict?: { local: FileInfo; remote: FileInfo };
  Delete?: string;
}

export interface SyncPlan {
  actions: SyncAction[];
  total_bytes: number;
  excluded: string[];
  resumed_files?: number;
}

export type Resolution = "KeepMine" | "UseTheirs" | "KeepBoth";

export interface ModProfile {
  id: string;
  name: string;
  description: string;
  icon: string;
  author: string;
  created_at: number;
  mods: ProfileMod[];
  game: Game;
}

export interface ProfileMod {
  relative_path: string;
  hash: string;
  size: number;
  name: string;
}

export interface SyncProgress {
  file: string;
  bytes_sent: number;
  bytes_total: number;
  files_done: number;
  files_total: number;
  peer_id?: string;
}

export interface PeerDownloadProgress {
  peer_id: string;
  peer_name: string;
  file: string | null;
  file_bytes_sent: number;
  file_bytes_total: number;
  files_sent: number;
}

export interface LogEntry {
  id: string;
  timestamp: number;
  message: string;
  level: "info" | "success" | "warning" | "error";
}

export interface ProfileComparison {
  profile_name: string;
  matched: number;
  missing: string[];
  modified: string[];
  extra: string[];
}

// Dynamic folder permissions: keys are content type IDs (e.g., "mods", "saves", "addons").
export type SyncFolderPermissions = Record<string, boolean>;

export type Page =
  | "dashboard"
  | "content"
  | "profiles"
  | "backups"
  | "activity"
  | "settings"
  | "game-browser";

export interface BackupInfo {
  id: string;
  created_at: number;
  label: string;
  file_count: number;
  total_size: number;
  /** Not currently serialized by the backend; prefer the *_count fields. */
  category_counts?: Record<string, number>;
  mods_count?: number;
  saves_count?: number;
  tray_count?: number;
  screenshots_count?: number;
  game: Game;
  auto?: boolean;
}

export interface AutoBackupConfig {
  auto_backup_before_sync: boolean;
  auto_backup_scheduled: boolean;
  auto_backup_interval_hours: number;
  auto_backup_max_count: number;
}

export interface SyncHistoryEntry {
  timestamp: number;
  game: string;
  peer_name: string;
  files_synced: number;
  total_bytes: number;
  errors: string[];
  direction: string;
  duration_ms: number;
}

export interface InstallResult {
  source: string;
  destination: string;
  status: "Success" | "Duplicate" | "InvalidExtension" | "Failed";
  message?: string;
}

// --- Game Registry types (mirrors backend GameDefinition) ---

export interface GameDefinition {
  id: string;
  label: string;
  family: string;
  icon: string;
  color: string;
  primary_color: string;
  auto_detect: boolean;
  detection?: DetectionConfig;
  validation?: ValidationConfig;
  content_types: ContentTypeDefinition[];
  dangerous_script_extensions: string[];
  packs?: string;
  legacy_id?: string;
  version_detection?: VersionDetection;
  path_correction?: PathCorrection;
  process_names?: string[];
  disable_method?: "folder" | "rename" | null;
  post_sync_delete?: string[];
}

export interface DetectionConfig {
  strategies: DetectionStrategy[];
  require_any?: string[];
}

export type DetectionStrategy =
  | { type: "documents_relative"; base: string; folders: string[] }
  | { type: "absolute_paths"; paths: PlatformPaths }
  | { type: "steam_library"; folders: string[] }
  | { type: "windows_registry"; keys: string[]; value: string; subpath?: string };

export interface PlatformPaths {
  windows: string[];
  macos: string[];
  linux: string[];
}

export interface ValidationConfig {
  check_dirs: string[];
  auto_create_dirs?: string[];
}

export interface ContentTypeDefinition {
  id: string;
  label: string;
  icon: string;
  color: string;
  folder: string;
  extensions: string[];
  file_type: string;
  classify_by_extension?: Record<string, string>;
  syncable?: boolean;
  recursive?: boolean;
  must_contain?: string | null;
  exclude_files?: string[];
}

export interface VersionDetection {
  file: string;
  method: string;
}

export interface PathCorrection {
  known_subfolders: string[];
  nested_corrections?: Record<string, number>;
}
