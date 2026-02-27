export interface FileInfo {
  relative_path: string;
  size: number;
  hash: string;
  modified: number;
  file_type: "Mod" | "CustomContent" | "Save" | "Tray" | "Screenshot";
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
  game: SimsGame;
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

export interface SyncFolderPermissions {
  mods: boolean;
  saves: boolean;
  tray: boolean;
  screenshots: boolean;
}

export type Page = "dashboard" | "mods" | "saves" | "profiles" | "backups" | "activity" | "settings";

export type SimsGame = "Sims2" | "Sims3" | "Sims4";

export interface BackupInfo {
  id: string;
  created_at: number;
  label: string;
  file_count: number;
  total_size: number;
  mods_count: number;
  saves_count: number;
  tray_count?: number;
  screenshots_count?: number;
  game: SimsGame;
}

export interface InstallResult {
  source: string;
  destination: string;
  status: "Success" | "Duplicate" | "InvalidExtension" | "Failed";
  message?: string;
}
