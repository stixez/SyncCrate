import { invoke } from "@tauri-apps/api/core";
import pkg from "../../package.json";
import { isDemoMode } from "./demoData";
import type {
  AutoPullResult,
  AutoBackupConfig,
  BackupInfo,
  ConnectionTestResult,
  FirewallStatus,
  NetworkDiagnostics,
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
  ModPack,
  OpenIntent,
  PackApplyPreview,
  PackApplyResult,
  PackApplyStatus,
  PackComparison,
  PackRevertResult,
  SyncFolderPermissions,
  SyncHistoryEntry,
  SyncPlan,
  UndoResult,
  UndoStatus,
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

/** Join a host using the join code shown on their screen (addresses + port + PIN). */
export async function connectByCode(code: string, name: string, pin?: string): Promise<SessionInfo> {
  return invoke("connect_by_code", { code, name, pin });
}

/** The host's join code for the current session. */
export async function getJoinCode(): Promise<string> {
  return invoke("get_join_code");
}

export async function getSessionStatus(): Promise<SessionStatus> {
  return invoke("get_session_status");
}

/** Client only: re-fetch the host's manifest and count files we'd download. */
export async function checkHostUpdates(): Promise<{ files: number; bytes: number }> {
  return invoke("check_host_updates");
}

export async function getCloseToTray(): Promise<boolean> {
  return invoke("get_close_to_tray");
}

export async function setCloseToTray(enabled: boolean): Promise<void> {
  return invoke("set_close_to_tray", { enabled });
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

export type GameArtKind = "cover" | "header" | "hero";

/** Cached Steam art (or the user's custom cover) as a data: URL; null if none. */
export async function getGameArt(gameId: string, kind: GameArtKind): Promise<string | null> {
  return invoke("get_game_art", { gameId, kind });
}

export async function setCustomGameArt(gameId: string, sourcePath: string): Promise<string> {
  return invoke("set_custom_game_art", { gameId, sourcePath });
}

export async function listCustomGameArt(): Promise<string[]> {
  return invoke("list_custom_game_art");
}

export async function clearCustomGameArt(gameId: string): Promise<void> {
  return invoke("clear_custom_game_art", { gameId });
}

export async function getAppVersion(): Promise<string> {
  // Demo mode has no backend; show the real version so screenshots don't say "v...".
  if (isDemoMode()) return pkg.version;
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

/** Rejects with a message starting with GAME_PATH_NEEDS_CONFIRMATION when the
 *  folder has none of the game's expected subfolders; retry with `force`. */
export async function setGamePath(game: string, path: string, force?: boolean): Promise<void> {
  return invoke("set_game_path", { game, path, force: force ?? null });
}

export const GAME_PATH_NEEDS_CONFIRMATION = "NEEDS_CONFIRMATION:";

/** Games whose saved folder is missing right now (e.g. unplugged drive). */
export async function getUnavailableGamePaths(): Promise<string[]> {
  if (isDemoMode()) return [];
  return invoke("get_unavailable_game_paths");
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

/** Refused unless `gameId` is the backend's active game. */
export async function toggleMod(
  gameId: string,
  relativePath: string,
  enabled: boolean,
): Promise<string> {
  return invoke("toggle_mod", { gameId, relativePath, enabled });
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

/** Ids of games actually installed (not just a leftover mods/saves folder). */
export async function getInstalledGames(): Promise<string[]> {
  return invoke("get_installed_games");
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

export async function getModTags(gameId: string): Promise<Record<string, string[]>> {
  return invoke("get_mod_tags", { gameId });
}

export async function setModTags(
  gameId: string,
  path: string,
  tags: string[],
): Promise<void> {
  return invoke("set_mod_tags", { gameId, path, tags });
}

export async function bulkSetTags(
  gameId: string,
  paths: string[],
  tags: string[],
): Promise<void> {
  return invoke("bulk_set_tags", { gameId, paths, tags });
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

export async function restoreBackup(id: string, exact = false): Promise<import("./types").RestoreResult> {
  return invoke("restore_backup", { id, exact });
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

// --- Undo last sync ---

export async function getUndoStatus(game?: string): Promise<UndoStatus | null> {
  return invoke("get_undo_status", { game: game ?? null });
}

export async function undoLastSync(game?: string): Promise<UndoResult> {
  return invoke("undo_last_sync", { game: game ?? null });
}

// --- Modpacks ---

export async function createPack(
  name: string,
  description: string,
  game?: string,
  contentTypes?: string[],
  paths?: string[],
  includeJoinCode = false,
): Promise<ModPack> {
  return invoke("create_pack", {
    game: game ?? null,
    contentTypes: contentTypes ?? null,
    paths: paths ?? null,
    name,
    description,
    includeJoinCode,
  });
}

export async function savePack(pack: ModPack, dest: string): Promise<void> {
  return invoke("save_pack", { pack, dest });
}

export async function packToLink(pack: ModPack): Promise<string> {
  return invoke("pack_to_link", { pack });
}

export async function loadPackFile(path: string): Promise<ModPack> {
  return invoke("load_pack_file", { path });
}

export async function loadPackLink(text: string): Promise<ModPack> {
  return invoke("load_pack_link", { text });
}

/** "Stay in sync": pull the host's new, non-script files (never replaces or deletes). */
export async function autoPull(): Promise<AutoPullResult> {
  return invoke("auto_pull");
}

/** Drain links/files opened from outside the app (queued by the backend). */
export async function takeOpenIntents(): Promise<OpenIntent[]> {
  return invoke("take_open_intents");
}

export async function comparePack(pack: ModPack): Promise<PackComparison> {
  return invoke("compare_pack", { pack });
}

export async function computePackSyncPlan(pack: ModPack, peerId?: string): Promise<SyncPlan> {
  return invoke("compute_pack_sync_plan", { peerId: peerId ?? null, pack });
}

export async function previewPackApply(pack: ModPack): Promise<PackApplyPreview> {
  return invoke("preview_pack_apply", { pack });
}

/** The disable/re-enable step; refused while pack files are still missing. */
export async function applyPackExact(pack: ModPack, preview: PackApplyPreview): Promise<PackApplyResult> {
  return invoke("apply_pack_exact", { pack, preview });
}

export async function getPackApplyStatus(game?: string): Promise<PackApplyStatus | null> {
  return invoke("get_pack_apply_status", { game: game ?? null });
}

export async function revertPackApply(game?: string): Promise<PackRevertResult> {
  return invoke("revert_pack_apply", { game: game ?? null });
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

// --- Transfer Speed Limit ---

export async function getTransferSpeedLimit(): Promise<number> {
  return invoke("get_transfer_speed_limit");
}

export async function setTransferSpeedLimit(limit: number): Promise<void> {
  return invoke("set_transfer_speed_limit", { limit });
}

// --- Sync History ---

export async function getSyncHistory(): Promise<SyncHistoryEntry[]> {
  return invoke("get_sync_history");
}

export async function clearSyncHistory(): Promise<void> {
  return invoke("clear_sync_history");
}

/** Median bytes/sec of recent syncs, or null when there is no usable history. */
export async function getTypicalTransferSpeed(): Promise<number | null> {
  return invoke("get_typical_transfer_speed");
}

export async function getClearCacheAfterSync(): Promise<boolean> {
  return invoke("get_clear_cache_after_sync");
}

export async function setClearCacheAfterSync(enabled: boolean): Promise<void> {
  return invoke("set_clear_cache_after_sync", { enabled });
}

// --- Game state ---

export async function checkGameRunning(game?: string): Promise<boolean> {
  return invoke("check_game_running", { game: game ?? null });
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

// --- Network / OS integration ---

export async function getFirewallStatus(): Promise<FirewallStatus> {
  return invoke("get_firewall_status");
}

/** Replaces SyncCrate's Windows Firewall rules with inbound allow rules (one UAC prompt). */
export async function fixFirewall(): Promise<FirewallStatus> {
  return invoke("fix_firewall");
}

export async function isElevated(): Promise<boolean> {
  return invoke("is_elevated");
}

/** Relaunches SyncCrate as administrator and exits this instance. */
export async function restartAsAdmin(): Promise<void> {
  return invoke("restart_as_admin");
}

export async function checkGamePathWritable(): Promise<boolean> {
  return invoke("check_game_path_writable");
}

export async function getNetworkDiagnostics(): Promise<NetworkDiagnostics> {
  return invoke("get_network_diagnostics");
}

export async function testConnection(ip: string, port: number): Promise<ConnectionTestResult> {
  return invoke("test_connection", { ip, port });
}

// --- Content maintenance ---

/** Mods still in a legacy `_Disabled/` folder (loaded anyway by The Sims). */
export async function countLegacyDisabled(game?: string): Promise<number> {
  return invoke("count_legacy_disabled", { game: game ?? null });
}

/** Move legacy `_Disabled/<rel>` mods to `<rel>.disabled`. */
export async function migrateLegacyDisabled(game?: string): Promise<import("./types").LegacyMigrationResult> {
  return invoke("migrate_legacy_disabled", { game: game ?? null });
}

/** Hashed scan + groups of identical files. */
export async function findDuplicates(game?: string): Promise<import("./types").DuplicateGroup[]> {
  return invoke("find_duplicates", { game: game ?? null });
}

/** Delete files inside the active game's content folders. `keep` maps each
 *  duplicate being deleted to the copy that stays (re-verified by the backend). */
export async function deleteModFiles(
  gameId: string,
  paths: string[],
  keep?: Record<string, string>,
): Promise<import("./types").DeleteResult> {
  return invoke("delete_mod_files", { gameId, paths, keep: keep ?? null });
}

/** Unix seconds of the last game patch (games with version detection only). */
export async function getGamePatchTime(game?: string): Promise<number | null> {
  return invoke("get_game_patch_time", { game: game ?? null });
}

/** Script mods of the active game older than its last patch. */
export async function getOutdatedScripts(gameId: string): Promise<import("./types").OutdatedScripts> {
  return invoke("get_outdated_scripts", { gameId });
}

/** Stop the running sync after the current file. Resolves false if nothing was syncing. */
export async function cancelSync(): Promise<boolean> {
  return invoke("cancel_sync");
}
