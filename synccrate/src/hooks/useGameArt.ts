import { useEffect, useState, useSyncExternalStore } from "react";
import * as cmd from "../lib/commands";
import type { GameArtKind } from "../lib/commands";
import { isDemoMode } from "../lib/demoData";
import { loadShowGameArt, saveShowGameArt } from "../lib/prefs";
import { useAppStore } from "../stores/useAppStore";

// One request per (game, kind) for the whole app: the backend call returns a
// data: URL, so re-asking on every render or for every tile would be wasteful.
const cache = new Map<string, Promise<string | null>>();

// Bumped when art changes (custom cover set/cleared, art toggled) so every
// mounted image re-resolves.
let version = 0;
const listeners = new Set<() => void>();
function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
function bump() {
  version++;
  listeners.forEach((fn) => fn());
}

let showArt = loadShowGameArt();

export function getShowGameArt() {
  return showArt;
}

export function setShowGameArt(show: boolean) {
  showArt = show;
  saveShowGameArt(show);
  bump();
}

/** Forget cached art for a game (after its custom cover changes). */
export function invalidateGameArt(gameId: string) {
  for (const key of cache.keys()) if (key.startsWith(`${gameId}:`)) cache.delete(key);
  bump();
}

const STEAM_FILE: Record<GameArtKind, string> = {
  cover: "library_600x900.jpg",
  header: "header.jpg",
  hero: "library_hero.jpg",
};

function load(gameId: string, kind: GameArtKind, steamAppId?: number): Promise<string | null> {
  const key = `${gameId}:${kind}`;
  let p = cache.get(key);
  if (!p) {
    // Demo mode runs in a plain browser without the backend (and without the
    // webview CSP), so point straight at Steam for screenshots.
    p = isDemoMode()
      ? Promise.resolve(
          steamAppId
            ? `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${steamAppId}/${STEAM_FILE[kind]}`
            : null,
        )
      : cmd.getGameArt(gameId, kind).catch(() => null);
    cache.set(key, p);
  }
  return p;
}

/**
 * Artwork URL for a game, or null while loading / when there is none (then
 * callers show the generated tile). Respects the "Show game art" setting.
 */
export function useGameArt(gameId: string | null | undefined, kind: GameArtKind): string | null {
  const v = useSyncExternalStore(subscribe, () => version);
  const steamAppId = useAppStore((s) => s.gameRegistry.find((g) => g.id === gameId)?.steam_app_id);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!gameId || !showArt) {
      setUrl(null);
      return;
    }
    let cancelled = false;
    load(gameId, kind, steamAppId).then((u) => {
      if (!cancelled) setUrl(u);
    });
    return () => {
      cancelled = true;
    };
  }, [gameId, kind, steamAppId, v]);

  return url;
}
