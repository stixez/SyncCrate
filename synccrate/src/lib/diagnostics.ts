import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { loadDiscordPresence, loadDisplayName } from "./prefs";
import { isDemoMode } from "./demoData";
import { toastError, toastSuccess } from "./toast";
import * as cmd from "./commands";

/** Same cap as the backend's MAX_LOG_LINES: only the tail is useful. */
const MAX_LINES = 30;

/** Build the "Copy diagnostics" report and put it on the clipboard. Nothing
 * is sent anywhere: the user pastes it into a GitHub issue themselves. */
export async function copyDiagnostics(): Promise<void> {
  const s = useAppStore.getState();
  const log = useLogStore
    .getState()
    .logs.filter((l) => l.level === "warning" || l.level === "error")
    .slice(-MAX_LINES)
    .map((l) => ({ timestamp: l.timestamp, level: l.level, message: l.message }));
  // Names the backend may not know (the last host after disconnecting, the
  // name typed but not used yet), so it can scrub them from the log too.
  const names = [
    loadDisplayName(),
    s.lastHostName ?? "",
    ...s.discoveredPeers.map((p) => p.name),
    ...(s.session?.peers ?? []).map((p) => p.name),
  ].filter((n) => n.trim().length > 0);
  try {
    const report = isDemoMode()
      ? "SyncCrate diagnostics (demo)\nPaste this into a GitHub issue: https://github.com/stixez/SyncCrate/issues/new/choose. It contains no files, codes or friends' names.\n"
      : await cmd.diagnosticsReport(log, s.stayInSync, loadDiscordPresence(), names);
    await navigator.clipboard.writeText(report);
    toastSuccess("Diagnostics copied");
  } catch (e) {
    toastError(`Couldn't copy diagnostics: ${e}`);
  }
}
