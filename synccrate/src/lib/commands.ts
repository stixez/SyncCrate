import { invoke } from "@tauri-apps/api/core";
import type {
  AutoBackupConfig,
  BackupInfo,
  FileManifest,
  GameDefinition,
  GameInfo,
  InstallResult,
  ModCompatibility,
  ModProfile,
  PeerInfo,
  ProfileComparison,
  Resolution,
  SessionInfo,
  SessionStatus,
  SyncFolderPermissions,
  SyncPlan,
} from "./types";

// --- Session ---

export async function startHost(
  name: string,
  usePin?: boolean,
  allowedFolders?: SyncFolderPermissions,
): Promise<SessionInfo> {
  return invoke("start_host", {
    name,
    usePin,
    allowedFolders: allowedFolders ?? null,
  });
}

export async function startJoin(name: string): Promise<PeerInfo[]> {
  return invoke("start_join", { name });
}

export async function connectToPeer(
  peerId: string,
  pin?: string,
): Promise<SessionInfo> {
  return invoke("connect_to_peer", { peerId, pin });
}

export async function disconnect(): Promise<void> {
  return invoke("disconnect");
}

export async function getSessionStatus(): Promise<SessionStatus> {
  return invoke("get_session_status");
}

export async function connectByIp(
  ip: string,
  port: number,
  name: string,
  pin?: string,
): Promise<SessionInfo> {
  return invoke("connect_by_ip", { ip, port, name, pin });
}

export async function disconnectPeer(peerId: string): Promise<void> {
  return invoke("disconnect_peer", { peerId });
}

export async function getAppVersion(): Promise<string> {
  return invoke("get_app_version");
}

export async function setSessionPort(port: number): Promise<void> {
  return invoke("set_session_port", { port });
}

export async function checkPortAvailable(port: number): Promise<boolean> {
  return invoke("check_port_available", { port });
}

// --- Files & Games ---

export async function scanFiles(
  game?: string,
  quick?: boolean,
): Promise<FileManifest> {
  return invoke("scan_files", { game: game ?? null, quick: quick ?? true });
}

export async function getGamePath(game: string): Promise<string> {
  return invoke("get_game_path", { game });
}

export async function setGamePath(game: string, path: string): Promise<void> {
  return invoke("set_game_path", { game, path });
}

export async function getActiveGame(): Promise<string> {
  return invoke("get_active_game");
}

export async function setActiveGame(game: string): Promise<void> {
  return invoke("set_active_game", { game });
}

export async function getAllGamePaths(): Promise<Record<string, string | null>> {
  return invoke("get_all_game_paths");
}

export async function openFolder(path: string): Promise<void> {
  return invoke("open_folder", { path });
}

export async function toggleMod(
  relativePath: string,
  enabled: boolean,
): Promise<string> {
  return invoke("toggle_mod", { relativePath, enabled });
}

// --- Game Registry & Library ---

export async function getGameRegistryCmd(): Promise<GameDefinition[]> {
  return invoke("get_game_registry");
}

export async function getUserLibrary(): Promise<string[]> {
  return invoke("get_user_library");
}

export async function addToLibrary(gameId: string): Promise<void> {
  return invoke("add_to_library", { gameId });
}

export async function removeFromLibrary(gameId: string): Promise<void> {
  return invoke("remove_from_library", { gameId });
}

export async function detectInstalledGames(): Promise<Record<string, string>> {
  return invoke("detect_installed_games");
}

// --- Sync ---

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

// --- Profiles ---

export async function listProfiles(): Promise<ModProfile[]> {
  return invoke("list_profiles");
}

export async function saveProfile(
  name: string,
  desc: string,
  icon: string,
  game?: string,
): Promise<ModProfile> {
  return invoke("save_profile", { name, desc, icon, game: game ?? null });
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

// --- Tags ---

export async function getPredefinedTags(): Promise<string[]> {
  return invoke("get_predefined_tags");
}

export async function getModTags(): Promise<Record<string, string[]>> {
  return invoke("get_mod_tags");
}

export async function setModTags(
  path: string,
  tags: string[],
): Promise<void> {
  return invoke("set_mod_tags", { path, tags });
}

export async function bulkSetTags(
  paths: string[],
  tags: string[],
): Promise<void> {
  return invoke("bulk_set_tags", { paths, tags });
}

// --- Install ---

export async function installModFiles(
  filePaths: string[],
  game?: string,
): Promise<InstallResult[]> {
  return invoke("install_mod_files", { filePaths, game: game ?? null });
}

export async function confirmInstallDuplicate(
  source: string,
  strategy: "overwrite" | "rename",
  game?: string,
): Promise<InstallResult> {
  return invoke("confirm_install_duplicate", {
    source,
    strategy,
    game: game ?? null,
  });
}

// --- Backup ---

export async function createBackup(
  label: string,
  game?: string,
): Promise<BackupInfo> {
  return invoke("create_backup", { label, game: game ?? null });
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

export async function renameBackup(
  id: string,
  label: string,
): Promise<void> {
  return invoke("rename_backup", { id, label });
}

// --- Auto-Backup Config ---

export async function getAutoBackupConfig(): Promise<AutoBackupConfig> {
  return invoke("get_auto_backup_config");
}

export async function setAutoBackupConfig(
  beforeSync: boolean,
  scheduled: boolean,
  intervalHours: number,
  maxCount: number,
): Promise<void> {
  return invoke("set_auto_backup_config", {
    beforeSync,
    scheduled,
    intervalHours,
    maxCount,
  });
}

// --- Packs ---

export async function detectPacks(game?: string): Promise<GameInfo> {
  return invoke("detect_packs", { game: game ?? null });
}

export async function getGameInfo(game?: string): Promise<GameInfo> {
  return invoke("get_game_info", { game: game ?? null });
}

export async function checkCompatibility(
  game?: string,
): Promise<ModCompatibility[]> {
  return invoke("check_compatibility", { game: game ?? null });
}
