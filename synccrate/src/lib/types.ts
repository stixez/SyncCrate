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
  /** Game the host is sharing (from discovery; absent for hosts before 0.5.6). */
  game_id?: string | null;
  /** Every address the host was discovered on, best first (`ip` is the first). */
  addresses?: string[];
  /** Host's iroh node id (hex), for recognising crew members; absent before 0.6.0. */
  node_id?: string | null;
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
  /** Host files skipped because they're outside this game's content folders. */
  skipped_foreign?: number;
  /** Game the host shares; null for hosts older than 0.5.6. */
  host_game?: string | null;
  /** Host files you have only as a disabled copy with the same content. */
  disabled_locally?: number;
  /** Host files the host has disabled while yours are enabled (left alone). */
  disabled_on_host?: number;
  warning?: string | null;
  /** Pack files (by path) the host doesn't have with the pack's exact hash. Only set on a pack sync plan. */
  pack_unavailable?: string[];
}

// --- Modpacks ---

export interface PackFile {
  relative_path: string;
  size: number;
  hash: string;
}

export interface PackJoin {
  code: string;
}

export interface ModPack {
  format_version: number;
  app_version: string;
  game_id: string;
  name: string;
  description: string;
  author: string;
  created_at: number;
  content_types: string[];
  join?: PackJoin | null;
  files: PackFile[];
}

/** What a clicked `synccrate://` link or opened `.scpack` file asked for
 * (validated by the backend; see `commands/open_intent.rs`). */
/** Mod info read from metadata files mods ship (src-tauri/src/mod_meta.rs). */
export interface ModMeta {
  /** The mod's folder, or the file itself when `is_file` (jars). */
  key: string;
  is_file: boolean;
  source: string;
  id?: string | null;
  name: string;
  version?: string | null;
  authors: string[];
  description?: string | null;
  website?: string | null;
  has_icon: boolean;
}

/** A previous version of a synced file (src-tauri/src/commands/history.rs). */
export interface FileVersion {
  id: string;
  path: string;
  hash: string;
  size: number;
  mtime_ms?: number | null;
  /** When it stopped being the current file (unix secs). */
  at: number;
  /** "replaced" | "deleted" | "before-restore" */
  reason: string;
  peer?: string;
  pending?: string | null;
}

export type OpenIntent =
  | { kind: "pack"; pack: ModPack }
  | { kind: "join"; code: string; game_id: string }
  | { kind: "crew"; invite: CrewInvite }
  | { kind: "invalid"; reason: string };

// Session chat (src-tauri/src/chat.rs). The host keeps the log; clients poll it.
export interface ChatMessage {
  seq: number;
  from: string;
  text: string;
  at: number;
  system?: boolean;
}

export interface ChatLog {
  messages: ChatMessage[];
  /** Typed on this client, not yet delivered to the host. */
  outbox: string[];
  /** False when not in a session, or the host is older than 0.6.0. */
  available: boolean;
}

// Crews (src-tauri/src/crews.rs). Local data; moves host → client during a session.
export interface CrewMember {
  node_id: string;
  name: string;
  updated_at?: number;
  removed?: boolean;
  last_seen?: number;
}

export interface CrewSet {
  pack: ModPack;
  version: number;
  published_at: number;
  publisher?: string;
}

export interface CrewHost {
  node_id: string;
  name: string;
  addresses?: string[];
  port?: number;
  game_id?: string;
  at: number;
}

export interface Crew {
  id: string;
  name: string;
  name_updated_at?: number;
  games: string[];
  members: CrewMember[];
  sets: Record<string, CrewSet>;
  last_host?: CrewHost | null;
  created_at?: number;
}

export interface CrewInvite {
  v: number;
  id: string;
  name: string;
  games: string[];
  from_node: string;
  from_name: string;
}

export interface CrewStatus {
  game_id: string;
  has_set: boolean;
  behind: number;
  comparison: PackComparison | null;
}

export interface CrewLanHost {
  crew_id: string;
  node_id: string;
  peer: PeerInfo;
}

export interface PackFileStatus {
  relative_path: string;
  size: number;
  content_type?: string | null;
}

export interface PackComparison {
  pack_name: string;
  pack_game: string;
  wrong_game: boolean;
  have: number;
  have_bytes: number;
  missing: PackFileStatus[];
  different: PackFileStatus[];
}

export interface PackApplyItem {
  relative_path: string;
  size: number;
  mtime_ms: number;
  hash: string;
  content_type?: string | null;
}

export interface SkippedFile {
  relative_path: string;
  reason: string;
}

/** "Apply pack exactly" preview; passed back unchanged to `applyPackExact`. */
export interface PackApplyPreview {
  game_id: string;
  base_path: string;
  pack_name: string;
  wrong_game: boolean;
  /** False for games that can't disable single files, or packs without mods. */
  available: boolean;
  unavailable_reason?: string | null;
  to_download: PackFileStatus[];
  conflicts: PackFileStatus[];
  to_enable: PackApplyItem[];
  to_disable: PackApplyItem[];
  /** Disabled pack files an enabled copy with other content blocks. Left alone. */
  blocked: SkippedFile[];
}

export interface PackApplyResult {
  enabled: number;
  disabled: number;
  skipped: SkippedFile[];
}

export interface PackApplyStatus {
  created_at: number;
  pack_name: string;
  disabled: number;
  enabled: number;
}

export interface PackRevertResult {
  reverted: number;
  skipped: SkippedFile[];
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
  | "modpacks"
  | "crews"
  | "activity"
  | "settings"
  | "game-browser";

export interface BackupInfo {
  id: string;
  created_at: number;
  label: string;
  file_count: number;
  total_size: number;
  /** Files per content type id. Empty for backups made before 0.6; use the *_count fields then. */
  category_counts?: Record<string, number>;
  /** "manual" | "auto" (scheduled) | "presync" (before sync, only replaced files) | "safety" (before a restore). */
  kind?: BackupKind;
  /** Bytes this backup added to the deduplicated store (absent for old full-copy backups). */
  new_bytes?: number;
  mods_count?: number;
  saves_count?: number;
  tray_count?: number;
  screenshots_count?: number;
  game: Game;
  auto?: boolean;
}

export type BackupKind = "manual" | "auto" | "presync" | "safety";

/** Payload of backup-progress / restore-progress. */
export interface BackupProgress {
  phase: BackupKind | "restore";
  game: string;
  file: string;
  files_done: number;
  files_total: number;
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
  /** The user stopped the sync before it finished. */
  cancelled?: boolean;
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
  /** Steam app id for store artwork; absent for non-Steam games. */
  steam_app_id?: number;
  /** Official publisher art for non-Steam games, by kind (cover/header/hero). */
  art_urls?: Record<string, string>;
  /** Genre tags (see GENRE_LABELS in lib/games.ts). */
  genres?: string[];
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
  /** "none": the game loads every subfolder and mods are folders, so no per-file toggle. */
  disable_method?: "folder" | "rename" | "none" | null;
  /** Duplicate finder offered (mods folder only). */
  duplicate_finder?: boolean;
  post_sync_delete?: string[];
  install_names?: string[];
  install_markers?: string[];
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

export interface LegacyMigrationResult {
  moved: number;
  collisions: string[];
  errors: string[];
}

export interface DuplicateGroup {
  hash: string;
  size: number;
  files: FileInfo[];
  wasted: number;
}

export interface DeleteResult {
  deleted: number;
  errors: string[];
}

/** Shown in the Backups area (and offered right after a sync) until another
 * sync, an undo, or a game/folder change invalidates it. */
export interface UndoStatus {
  created_at: number;
  added: number;
  replaced: number;
  deleted: number;
}

export interface UndoResult {
  /** Replaced/deleted files put back from the presync backup. */
  restored: number;
  /** Added files (including "keep both" `_remote` copies) removed. */
  removed: number;
  /** Left alone, with why (changed since the sync, or no backup for it). */
  skipped: string[];
}

export interface RestoreResult {
  restored: number;
  /** Already identical (same size and modification time). */
  unchanged: number;
  /** Files left alone because a disabled/enabled copy exists now. */
  skipped: string[];
  /** Entries whose stored data is missing or whose content type no longer exists. */
  missing: number;
  /** Files removed by an exact restore. */
  removed: number;
  /** Set when the restore stopped part-way; `restored` files were already written. */
  error: string | null;
  /** Label of the safety backup holding the previous files (null if there was nothing to save). */
  safety_backup: string | null;
}

export interface OutdatedScripts {
  patch_time: number | null;
  paths: string[];
}
