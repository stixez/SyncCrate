import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";
import type { Resolution } from "../lib/types";

export function useSync() {
  const [isLoading, setIsLoading] = useState(false);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const addLog = useLogStore((s) => s.addLog);

  const computePlan = async () => {
    setIsLoading(true);
    try {
      const plan = await cmd.computeSyncPlan();
      setSyncPlan(plan);
      addLog(`Sync plan: ${plan.actions.length} actions`, "info");
    } catch (e: any) {
      addLog(`Failed to compute sync plan: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  const executeSync = async () => {
    setIsLoading(true);
    try {
      await cmd.executeSync();
    } catch (e: any) {
      addLog(`Sync failed: ${e}`, "error");
    } finally {
      setIsLoading(false);
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

  return { computePlan, executeSync, resolve, resolveAll, isLoading };
}
