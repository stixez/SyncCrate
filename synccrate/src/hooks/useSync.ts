import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { toastError, toastInfo, toastSuccess } from "../lib/toast";
import { incrementSyncCount, checkMilestone } from "../lib/donations";
import * as cmd from "../lib/commands";
import type { Resolution } from "../lib/types";
import { friendlyError } from "../lib/errors";
import { displayPath, plural } from "../lib/utils";

export function useSync() {
  const [isLoading, setIsLoading] = useState(false);
  // Only a started sync (not a compare): the banner's "Getting ready".
  const [isStarting, setIsStarting] = useState(false);
  const [loadingPhase, setLoadingPhase] = useState("");
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const addLog = useLogStore((s) => s.addLog);

  const computePlan = async () => {
    setIsLoading(true);
    try {
      // Full scan with hashes needed for accurate sync comparison
      setLoadingPhase("Checking your files...");
      await cmd.scanFiles(undefined, false);
      setLoadingPhase("Comparing with the host...");
      const plan = await cmd.computeSyncPlan();
      // A new compare replaces any pack plan, so a pending "apply pack
      // exactly" must not fire on this (unrelated) sync's completion. Only
      // once the plan exists: clearing it first let a refused compare (a
      // sync already running) silently drop the pack's disable step.
      useAppStore.getState().setPendingPackApply(null);
      setSyncPlan(plan);
      const count = plan.actions.length;
      if (plan.warning) {
        // e.g. an older host that seems to share a different game
        addLog(plan.warning, "warning");
        toastError(plan.warning);
      } else if (count > 0) {
        toastSuccess(`Found ${plural(count, "difference")} to sync`);
      } else {
        toastSuccess("Everything is in sync");
      }
      addLog(`Sync plan: ${count} actions`, "info");
    } catch (e: any) {
      addLog(`Failed to compute sync plan: ${e}`, "error");
      toastError(`Sync failed: ${e}`);
    } finally {
      setIsLoading(false);
      setLoadingPhase("");
    }
  };

  const executeSync = async () => {
    setIsLoading(true);
    setIsStarting(true);
    setLoadingPhase("Syncing files...");
    try {
      await cmd.executeSync();
      const count = incrementSyncCount();
      const milestone = checkMilestone(count);
      if (milestone) {
        useAppStore.getState().setDonationMilestone(milestone);
      }
    } catch (e: any) {
      // Early backend errors return before `sync-complete` fires; don't leave
      // the progress bar (or a failed presync backup's counts) stuck.
      useAppStore.getState().setSyncProgress(null);
      if (useAppStore.getState().backupProgress?.phase === "presync") useAppStore.getState().setBackupProgress(null);
      if (String(e).includes("Game folder changed")) {
        // The backend dropped the stale plan; don't offer to run it again.
        setSyncPlan(null);
        toastError(`${e}`);
      } else if (String(e).includes("Sync cancelled")) {
        // The sync-complete listener says it (with Undo if files arrived).
      } else if (/file\(s\) failed to sync/.test(String(e)) && useAppStore.getState().session?.session_type === "Client") {
        // Partly done: sync-complete reports it (with the details in the log
        // and an Undo for what did arrive); a second, red "Sync failed" toast
        // on top of that contradicted it. Not if the host dropped: then the
        // session is gone and that report never comes.
      } else {
        addLog(`Sync failed: ${e}`, "error");
        toastError(`Sync failed: ${e}`);
      }
    } finally {
      setIsLoading(false);
      setIsStarting(false);
      setLoadingPhase("");
    }
  };

  const resolve = async (path: string, resolution: Resolution) => {
    try {
      const updatedPlan = await cmd.resolveConflict(path, resolution);
      setSyncPlan(updatedPlan);
      addLog(`Resolved conflict for ${displayPath(path)}: ${resolution}`, "success");
    } catch (e: any) {
      addLog(`Failed to resolve conflict: ${e}`, "error");
      toastError(`Couldn't resolve the conflict: ${friendlyError(e)}`);
    }
  };

  const resolveAll = async (strategy: string) => {
    try {
      const updatedPlan = await cmd.resolveAllConflicts(strategy);
      setSyncPlan(updatedPlan);
      addLog(`Resolved all conflicts using "${strategy}"`, "success");
    } catch (e: any) {
      addLog(`Failed to resolve all conflicts: ${e}`, "error");
      toastError(`Couldn't resolve the conflicts: ${friendlyError(e)}`);
    }
  };

  return { computePlan, executeSync, resolve, resolveAll, isLoading, isStarting, loadingPhase };
}
