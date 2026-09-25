import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAppStore, type ConnectAttempt } from "../stores/useAppStore";
import { loadDisplayName } from "../lib/prefs";
import { toastError, toastInfo, toastSuccess } from "../lib/toast";
import { isDemoMode } from "../lib/demoData";
import type { OpenIntent } from "../lib/types";
import * as cmd from "../lib/commands";

/** Show what a clicked link / opened file asked for. Never acts on its own:
 * a pack opens the import view (comparing is read-only), an invite fills the
 * join box or raises the existing wrong-game prompt; connecting, switching
 * games and syncing all still need a click. */
function routeOpenIntent(intent: OpenIntent) {
  const s = useAppStore.getState();
  if (intent.kind === "invalid") {
    toastError(intent.reason);
    return;
  }
  if (s.syncProgress) {
    toastInfo("Finish the current sync first, then open the link again.");
    return;
  }
  const game = s.activeGame;
  if (intent.kind === "pack") {
    // The pack view compares against the active game and offers the switch
    // itself when the pack is for another one.
    s.navigateToGame(game, "modpacks");
    s.setPendingImportPack(intent.pack);
    return;
  }
  if (s.session && s.session.session_type !== "None") {
    toastInfo("You're already in a session. Disconnect first, then open the invite again.");
    return;
  }
  s.navigateToGame(game, "dashboard");
  if (intent.game_id !== game) {
    const attempt: ConnectAttempt = { kind: "code", code: intent.code, name: loadDisplayName().trim() || "Guest", label: "host" };
    s.setGameSwitchPrompt({ hostGame: intent.game_id, attempt });
  } else {
    s.setPendingJoinCode(intent.code);
    toastSuccess("Invite opened — click Join to connect");
  }
}

/** Drains the backend's queue once the app is ready (cold start: the link
 * that launched us) and again on every `open-intent` nudge (warm start). */
export function useOpenIntents(ready: boolean) {
  useEffect(() => {
    if (!ready || isDemoMode()) return;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    // Routing isn't gated on `disposed`: the backend queue is already drained,
    // so dropping the result on a StrictMode remount would lose the link.
    const drain = () => cmd.takeOpenIntents().then((list) => list.forEach(routeOpenIntent)).catch(() => {});
    listen("open-intent", drain).then((u) => {
      if (disposed) u();
      else unlisten = u;
      // After the listener exists, so nothing queued in between is missed.
      drain();
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [ready]);
}
