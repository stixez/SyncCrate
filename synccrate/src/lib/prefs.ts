import type { SyncFolderPermissions } from "./types";

// Small per-user preferences remembered in localStorage (never required: every
// accessor tolerates storage being unavailable).

const NAME_KEY = "synccrate-display-name";
const USE_PIN_KEY = "synccrate-host-use-pin";
const FOLDER_PERMS_KEY = "synccrate-folder-perms";
const GAME_ART_KEY = "synccrate-game-art";

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

/** Show game artwork (downloaded from Steam on first use). On by default. */
export function loadShowGameArt(): boolean {
  return read(GAME_ART_KEY) !== "0";
}

export function saveShowGameArt(show: boolean) {
  write(GAME_ART_KEY, show ? "1" : "0");
}

// ---------- appearance ----------

const APPEARANCE_KEY = "synccrate-appearance";
const THEME_KEY = "synccrate-theme";

export type ThemeMode = "dark" | "light" | "system";
export type UiScale = 0.9 | 1 | 1.1 | 1.25;
export type Density = "comfortable" | "compact";

export interface Appearance {
  /** User accent as #rrggbb. The brand mint renders the exact website palette. */
  accent: string;
  /** Recolor the UI with the selected game's color instead of `accent`. */
  matchGame: boolean;
  scale: UiScale;
  density: Density;
  /** null = follow the OS (off when it asks for reduced motion). */
  effects: boolean | null;
}

export const DEFAULT_APPEARANCE: Appearance = {
  accent: "#1fb87e",
  matchGame: false,
  scale: 1,
  density: "comfortable",
  effects: null,
};

const SCALES: UiScale[] = [0.9, 1, 1.1, 1.25];

/** Stored appearance, with every field validated so a hand-edited or old value can't break the UI. */
export function loadAppearance(): Appearance {
  let raw: Partial<Appearance> = {};
  try {
    const parsed = JSON.parse(read(APPEARANCE_KEY) ?? "{}");
    if (parsed && typeof parsed === "object") raw = parsed;
  } catch {
    // corrupt value — fall back to defaults
  }
  const d = DEFAULT_APPEARANCE;
  return {
    accent: typeof raw.accent === "string" && /^#[0-9a-f]{6}$/i.test(raw.accent) ? raw.accent.toLowerCase() : d.accent,
    matchGame: typeof raw.matchGame === "boolean" ? raw.matchGame : d.matchGame,
    scale: SCALES.includes(raw.scale as UiScale) ? (raw.scale as UiScale) : d.scale,
    density: raw.density === "compact" ? "compact" : d.density,
    effects: typeof raw.effects === "boolean" ? raw.effects : null,
  };
}

export function saveAppearance(a: Appearance) {
  write(APPEARANCE_KEY, JSON.stringify(a));
}

export function loadThemeMode(): ThemeMode {
  const t = read(THEME_KEY);
  return t === "light" || t === "system" ? t : "dark";
}

export function saveThemeMode(mode: ThemeMode) {
  write(THEME_KEY, mode);
}
