import * as cmd from "./commands";
import { toastError, toastInfo, toastSuccess } from "./toast";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { ModPack, PackApplyPreview } from "./types";

/** Run the disable/re-enable step of "apply pack exactly" and report it.
 * Shared by ModpackList (nothing to download) and the sync-complete
 * handler (after the pack's download). Resolves to whether it ran. */
export async function runPackApply(pack: ModPack, preview: PackApplyPreview): Promise<boolean> {
  const addLog = useLogStore.getState().addLog;
  try {
    const r = await cmd.applyPackExact(pack, preview);
    const summary = `${r.disabled} disabled, ${r.enabled} re-enabled${r.skipped.length ? `, ${r.skipped.length} skipped` : ""}`;
    addLog(`Pack "${pack.name}" applied: ${summary}`, r.skipped.length ? "warning" : "success");
    for (const s of r.skipped) addLog(`  Skipped ${s.relative_path}: ${s.reason}`, "warning");
    if (r.skipped.length) toastInfo(`Pack applied: ${summary}. See the activity log for skipped files.`);
    else toastSuccess(`Pack applied: ${summary}`);
    try {
      useAppStore.getState().setManifest(await cmd.scanFiles(preview.game_id));
    } catch {}
    return true;
  } catch (e) {
    addLog(`Apply pack exactly stopped: ${e}`, "error");
    toastError(`${e}`);
    return false;
  }
}
