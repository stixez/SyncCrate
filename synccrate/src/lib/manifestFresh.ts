import { useAppStore } from "../stores/useAppStore";

/**
 * The active game's list is kept current by the file watcher, so a page
 * visit doesn't need a full rescan: rescanning (and sending over) 30k files
 * on every Dashboard/Content switch made changing pages slow. Older than
 * this, or another game's list, it rescans anyway (in case the watcher
 * missed something, e.g. on a network drive).
 */
const FRESH_MS = 10 * 60 * 1000;

export function manifestIsFresh(gameId: string): boolean {
  const s = useAppStore.getState();
  return !!s.manifest && s.manifestGame === gameId && s.activeGame === gameId && Date.now() - s.manifestAt < FRESH_MS;
}
