import { ask } from "@tauri-apps/plugin-dialog";
import * as cmd from "./commands";
import { gameLabel } from "./games";
import { useAppStore } from "../stores/useAppStore";

/**
 * Save a game folder. When the backend says the folder doesn't look like the
 * game's (none of its expected subfolders), asks before using it anyway.
 * Re-reads all paths afterwards, because the backend stores the canonical
 * path (and may climb out of a picked `Mods` subfolder), not what was picked.
 * Returns the stored path, or null if the user cancelled.
 */
export async function saveGamePath(gameId: string, path: string): Promise<string | null> {
  try {
    await cmd.setGamePath(gameId, path);
  } catch (e) {
    const msg = String(e);
    if (!msg.startsWith(cmd.GAME_PATH_NEEDS_CONFIRMATION)) throw e;
    const ok = await ask(msg.slice(cmd.GAME_PATH_NEEDS_CONFIRMATION.length).trim(), {
      title: gameLabel(gameId),
      kind: "warning",
      okLabel: "Use it anyway",
      cancelLabel: "Cancel",
    });
    if (!ok) return null;
    await cmd.setGamePath(gameId, path, true);
  }
  const all = await cmd.getAllGamePaths();
  const paths: Record<string, string> = {};
  for (const [id, p] of Object.entries(all)) if (p) paths[id] = p;
  useAppStore.getState().setGamePaths(paths);
  return paths[gameId] ?? path;
}
