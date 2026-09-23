import type { SyncFolderPermissions } from "./types";

// Small per-user preferences remembered in localStorage (never required: every
// accessor tolerates storage being unavailable).

const NAME_KEY = "synccrate-display-name";
const USE_PIN_KEY = "synccrate-host-use-pin";
const FOLDER_PERMS_KEY = "synccrate-folder-perms";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable — ignore
  }
}

export function loadDisplayName(): string {
  return read(NAME_KEY) ?? "";
}

export function saveDisplayName(name: string) {
  write(NAME_KEY, name);
}

export function loadUsePin(): boolean {
  return read(USE_PIN_KEY) === "1";
}

export function saveUsePin(usePin: boolean) {
  write(USE_PIN_KEY, usePin ? "1" : "0");
}

function loadAllFolderPerms(): Record<string, SyncFolderPermissions> {
  try {
    const parsed = JSON.parse(read(FOLDER_PERMS_KEY) ?? "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

/** Saved share permissions for a game (content type id -> shared), if any. */
export function loadFolderPerms(gameId: string): SyncFolderPermissions {
  const perms = loadAllFolderPerms()[gameId];
  return perms && typeof perms === "object" ? perms : {};
}

export function saveFolderPerms(gameId: string, perms: SyncFolderPermissions) {
  const all = loadAllFolderPerms();
  all[gameId] = perms;
  write(FOLDER_PERMS_KEY, JSON.stringify(all));
}
