import { invoke } from "@tauri-apps/api/core";
import type {
  BackupInfo,
  FileManifest,
  InstallResult,
  ModProfile,
  PeerInfo,
  ProfileComparison,
  Resolution,
  SessionInfo,
  SessionStatus,
  SimsGame,
  SyncPlan,
} from "./types";

export async function startHost(name: string, usePin?: boolean): Promise<SessionInfo> {
  return invoke("start_host", { name, usePin });
}

export async function startJoin(name: string): Promise<PeerInfo[]> {
  return invoke("start_join", { name });
}

export async function connectToPeer(peerId: string, pin?: string): Promise<SessionInfo> {
  return invoke("connect_to_peer", { peerId, pin });
}

export async function disconnect(): Promise<void> {
  return invoke("disconnect");
}

export async function getSessionStatus(): Promise<SessionStatus> {
  return invoke("get_session_status");
}

export async function scanFiles(): Promise<FileManifest> {
  return invoke("scan_files");
}

export async function getSims4Path(): Promise<string> {
  return invoke("get_sims4_path");
}

export async function setSims4Path(path: string): Promise<void> {
  return invoke("set_sims4_path", { path });
}

export async function computeSyncPlan(peerId?: string): Promise<SyncPlan> {
  return invoke("compute_sync_plan", { peerId });
}

export async function executeSync(peerId?: string): Promise<void> {
  return invoke("execute_sync", { peerId });
}

export async function resolveConflict(
  path: string,
  resolution: Resolution,
  peerId?: string,
): Promise<SyncPlan> {
  return invoke("resolve_conflict", { path, resolution, peerId });
}

export async function resolveAllConflicts(
  strategy: string,
  peerId?: string,
): Promise<SyncPlan> {
  return invoke("resolve_all_conflicts", { strategy, peerId });
}

export async function disconnectPeer(peerId: string): Promise<void> {
  return invoke("disconnect_peer", { peerId });
}

export async function listProfiles(): Promise<ModProfile[]> {
  return invoke("list_profiles");
}

export async function saveProfile(
  name: string,
  desc: string,
  icon: string,
): Promise<ModProfile> {
  return invoke("save_profile", { name, desc, icon });
}

export async function loadProfile(id: string): Promise<ProfileComparison> {
  return invoke("load_profile", { id });
}

export async function exportProfile(
  id: string,
  dest: string,
): Promise<void> {
  return invoke("export_profile", { id, dest });
}

export async function importProfile(path: string): Promise<ModProfile> {
  return invoke("import_profile", { path });
}

export async function deleteProfile(id: string): Promise<void> {
  return invoke("delete_profile", { id });
}

export async function getAppVersion(): Promise<string> {
  return invoke("get_app_version");
}

export async function setSessionPort(port: number): Promise<void> {
  return invoke("set_session_port", { port });
}

// Tags
export async function getPredefinedTags(): Promise<string[]> {
  return invoke("get_predefined_tags");
}

export async function getModTags(): Promise<Record<string, string[]>> {
  return invoke("get_mod_tags");
}

export async function setModTags(path: string, tags: string[]): Promise<void> {
  return invoke("set_mod_tags", { path, tags });
}

export async function bulkSetTags(paths: string[], tags: string[]): Promise<void> {
  return invoke("bulk_set_tags", { paths, tags });
}

// Install
export async function installModFiles(filePaths: string[]): Promise<InstallResult[]> {
  return invoke("install_mod_files", { filePaths });
}

export async function confirmInstallDuplicate(
  source: string,
  strategy: "overwrite" | "rename",
): Promise<InstallResult> {
  return invoke("confirm_install_duplicate", { source, strategy });
}

// Backup
export async function createBackup(label: string): Promise<BackupInfo> {
  return invoke("create_backup", { label });
}

export async function listBackups(): Promise<BackupInfo[]> {
  return invoke("list_backups");
}

export async function restoreBackup(id: string): Promise<void> {
  return invoke("restore_backup", { id });
}

export async function deleteBackup(id: string): Promise<void> {
  return invoke("delete_backup", { id });
}

// Profile (updated)
export async function saveProfileWithGame(
  name: string,
  desc: string,
  icon: string,
  game: SimsGame,
): Promise<ModProfile> {
  return invoke("save_profile", { name, desc, icon, game });
}

// Selective Sync
export async function updateSyncSelection(
  peerId: string,
  excludedPaths: string[],
): Promise<SyncPlan> {
  return invoke("update_sync_selection", { peerId, excludedPaths });
}

export async function setExcludePatterns(patterns: string[]): Promise<void> {
  return invoke("set_exclude_patterns", { patterns });
}

export async function getExcludePatterns(): Promise<string[]> {
  return invoke("get_exclude_patterns");
}
