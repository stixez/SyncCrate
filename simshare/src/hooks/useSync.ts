import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { toastError, toastSuccess } from "../lib/toast";
import { incrementSyncCount, checkMilestone } from "../lib/donations";
import * as cmd from "../lib/commands";
import type { Resolution } from "../lib/types";

export function useSync() {
  const [isLoading, setIsLoading] = useState(false);
  const [loadingPhase, setLoadingPhase] = useState("");
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const addLog = useLogStore((s) => s.addLog);

  const computePlan = async () => {
    setIsLoading(true);
    try {
      // Full scan with hashes needed for accurate sync comparison
      setLoadingPhase("Hashing files...");
      await cmd.scanFiles(undefined, false);
      setLoadingPhase("Comparing manifests...");
      const plan = await cmd.computeSyncPlan();
      setSyncPlan(plan);
      const count = plan.actions.length;
      if (count > 0) {
        toastSuccess(`Found ${count} difference(s) to sync`);
      } else {
        toastSuccess("Everything is in sync!");
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
    setLoadingPhase("Syncing files...");
    try {
      await cmd.executeSync();
      toastSuccess("Sync complete!");
      const count = incrementSyncCount();
      const milestone = checkMilestone(count);
      if (milestone) {
        useAppStore.getState().setDonationMilestone(milestone);
      }
    } catch (e: any) {
      addLog(`Sync failed: ${e}`, "error");
      toastError(`Sync failed: ${e}`);
    } finally {
      setIsLoading(false);
      setLoadingPhase("");
    }
  };

  const resolve = async (path: string, resolution: Resolution) => {
    try {
      const updatedPlan = await cmd.resolveConflict(path, resolution);
      setSyncPlan(updatedPlan);
      addLog(`Resolved conflict for ${path}: ${resolution}`, "success");
    } catch (e: any) {
      addLog(`Failed to resolve conflict: ${e}`, "error");
    }
  };

  const resolveAll = async (strategy: string) => {
    try {
      const updatedPlan = await cmd.resolveAllConflicts(strategy);
      setSyncPlan(updatedPlan);
      addLog(`Resolved all conflicts using "${strategy}"`, "success");
    } catch (e: any) {
      addLog(`Failed to resolve all conflicts: ${e}`, "error");
    }
  };

  return { computePlan, executeSync, resolve, resolveAll, isLoading, loadingPhase };
}
