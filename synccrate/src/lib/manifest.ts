import { useAppStore } from "../stores/useAppStore";
import { isDemoMode } from "./demoData";

/** Whether the store's manifest is already this game's current file list:
 * it's cleared whenever the selected game changes, and the file watcher
 * rescans the active game when its files change. Pages opening for that
 * game then skip their own full rescan (150k files walked and sent over IPC
 * on every page switch). */
export function manifestIsCurrent(gameId: string): boolean {
  const s = useAppStore.getState();
  return !isDemoMode() && s.manifest !== null && s.selectedGame === gameId && s.activeGame === gameId;
}
