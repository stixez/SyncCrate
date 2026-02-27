import { useRef, useEffect, useState, useMemo, useCallback } from "react";
import { ArrowUpDown, AlertTriangle, ChevronDown, ChevronUp } from "lucide-react";
import type { SyncPlan } from "../lib/types";
import { formatBytes } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import SyncActionItem from "./SyncActionItem";
import * as cmd from "../lib/commands";

interface SyncBannerProps {
  plan: SyncPlan;
  onSync: () => void;
  onResolveAll?: (strategy: string) => void;
}

function formatEta(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "--";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.ceil(seconds % 60);
  return `${m}m ${s}s`;
}

export default function SyncBanner({ plan, onSync, onResolveAll }: SyncBannerProps) {
  const syncProgress = useAppStore((s) => s.syncProgress);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const session = useAppStore((s) => s.session);
  const startTimeRef = useRef<number | null>(null);
  const startBytesRef = useRef<number>(0);
  const [expanded, setExpanded] = useState(false);
  const [quickFilter, setQuickFilter] = useState<string | null>(null);

  useEffect(() => {
    if (syncProgress && startTimeRef.current === null) {
      startTimeRef.current = Date.now();
      startBytesRef.current = syncProgress.bytes_sent;
    }
    if (!syncProgress) {
      startTimeRef.current = null;
      startBytesRef.current = 0;
    }
  }, [syncProgress]);

  const excluded = useMemo(() => new Set(plan.excluded || []), [plan.excluded]);

  const sendCount = plan.actions.filter((a) => a.SendToRemote).length;
  const receiveCount = plan.actions.filter((a) => a.ReceiveFromRemote).length;
  const conflictCount = plan.actions.filter((a) => a.Conflict).length;

  let speedText = "";
  let etaText = "";
  if (syncProgress && startTimeRef.current) {
    const elapsed = (Date.now() - startTimeRef.current) / 1000;
    const bytesDelta = syncProgress.bytes_sent - startBytesRef.current;
    if (elapsed > 1) {
      const speed = bytesDelta / elapsed;
      speedText = `${formatBytes(speed)}/s`;
      const remaining = syncProgress.bytes_total - syncProgress.bytes_sent;
      if (speed > 0) {
        etaText = formatEta(remaining / speed);
      }
    }
  }

  const setPage = useAppStore((s) => s.setPage);

  const modConflicts = plan.actions.filter(
    (a) => a.Conflict && (a.Conflict.local.file_type === "Mod" || a.Conflict.local.file_type === "CustomContent"),
  ).length;
  const saveConflicts = plan.actions.filter(
    (a) => a.Conflict && a.Conflict.local.file_type === "Save",
  ).length;

  const toggleExclusion = useCallback(
    async (path: string) => {
      const peerId = session?.peers?.[0]?.id;
      if (!peerId) return;

      const newExcluded = excluded.has(path)
        ? Array.from(excluded).filter((p) => p !== path)
        : [...Array.from(excluded), path];

      try {
        const updated = await cmd.updateSyncSelection(peerId, newExcluded);
        setSyncPlan(updated);
      } catch (e) {
        console.error("Failed to update selection:", e);
      }
    },
    [excluded, session, setSyncPlan],
  );

  const applyQuickFilter = useCallback(
    async (filter: string) => {
      const peerId = session?.peers?.[0]?.id;
      if (!peerId) return;

      let newExcluded: string[];
      if (filter === "select_all") {
        newExcluded = [];
      } else if (filter === "deselect_all") {
        newExcluded = plan.actions.map((a) => {
          if (a.SendToRemote) return a.SendToRemote.relative_path;
          if (a.ReceiveFromRemote) return a.ReceiveFromRemote.relative_path;
          if (a.Conflict) return a.Conflict.local.relative_path;
          if (a.Delete) return a.Delete;
          return "";
        }).filter(Boolean);
      } else if (filter === "mods_only") {
        newExcluded = plan.actions
          .filter((a) => {
            const ft = a.SendToRemote?.file_type || a.ReceiveFromRemote?.file_type || a.Conflict?.local.file_type;
            return ft === "Save";
          })
          .map((a) => {
            if (a.SendToRemote) return a.SendToRemote.relative_path;
            if (a.ReceiveFromRemote) return a.ReceiveFromRemote.relative_path;
            if (a.Conflict) return a.Conflict.local.relative_path;
            return "";
          })
          .filter(Boolean);
      } else if (filter === "saves_only") {
        newExcluded = plan.actions
          .filter((a) => {
            const ft = a.SendToRemote?.file_type || a.ReceiveFromRemote?.file_type || a.Conflict?.local.file_type;
            return ft !== "Save";
          })
          .map((a) => {
            if (a.SendToRemote) return a.SendToRemote.relative_path;
            if (a.ReceiveFromRemote) return a.ReceiveFromRemote.relative_path;
            if (a.Conflict) return a.Conflict.local.relative_path;
            return "";
          })
          .filter(Boolean);
      } else {
        return;
      }

      setQuickFilter(filter);
      try {
        const updated = await cmd.updateSyncSelection(peerId, newExcluded);
        setSyncPlan(updated);
      } catch (e) {
        console.error("Failed to apply filter:", e);
      }
    },
    [plan, session, setSyncPlan],
  );

  const excludedCount = excluded.size;

  return (
    <div className="bg-accent/10 border border-accent/30 rounded-xl p-4">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <ArrowUpDown size={16} className="text-accent-light" />
          <span className="font-medium text-sm">Sync Plan Ready</span>
          {excludedCount > 0 && (
            <span className="text-[10px] text-txt-dim">({excludedCount} excluded)</span>
          )}
        </div>
        {conflictCount === 0 ? (
          <button
            onClick={onSync}
            disabled={!!syncProgress}
            className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-1.5 text-sm font-medium transition-colors disabled:opacity-50"
          >
            {syncProgress ? "Syncing..." : "Sync Now"}
          </button>
        ) : (
          <div className="flex items-center gap-2">
            {onResolveAll && (
              <button
                onClick={() => onResolveAll("use_newest")}
                disabled={!!syncProgress}
                className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-1.5 text-sm font-medium transition-colors disabled:opacity-50"
              >
                Resolve All: Use Newest
              </button>
            )}
            <button
              onClick={() => setPage(modConflicts > 0 ? "mods" : "saves")}
              className="bg-bg-card border border-border hover:bg-bg-card-hover text-txt rounded-lg px-4 py-1.5 text-sm font-medium transition-colors"
            >
              View Conflicts
            </button>
          </div>
        )}
      </div>
      {conflictCount > 0 && (
        <div className="flex items-center gap-2 mb-2 text-status-yellow text-xs">
          <AlertTriangle size={14} className="shrink-0" />
          <span>
            {conflictCount} conflict{conflictCount !== 1 ? "s" : ""} must be resolved before syncing
            {modConflicts > 0 && saveConflicts > 0 && ` (${modConflicts} mod, ${saveConflicts} save)`}
          </span>
        </div>
      )}
      <div className="flex items-center gap-4 text-xs text-txt-dim">
        <div className="flex gap-4 flex-1">
          {sendCount > 0 && <span>Upload: {sendCount} files</span>}
          {receiveCount > 0 && <span>Download: {receiveCount} files</span>}
          {conflictCount > 0 && <span className="text-status-red">Conflicts: {conflictCount}</span>}
          <span>Total: {formatBytes(plan.total_bytes)}</span>
        </div>
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1 text-accent-light hover:text-accent text-xs"
        >
          {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          {expanded ? "Hide" : "Details"}
        </button>
      </div>

      {expanded && (
        <div className="mt-3 border-t border-accent/20 pt-3">
          <div className="flex gap-1.5 mb-2 flex-wrap">
            {["select_all", "deselect_all", "mods_only", "saves_only"].map((f) => (
              <button
                key={f}
                onClick={() => applyQuickFilter(f)}
                className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${
                  quickFilter === f
                    ? "bg-accent text-white"
                    : "bg-bg border border-border text-txt-dim hover:border-accent/50"
                }`}
              >
                {f === "select_all"
                  ? "Select All"
                  : f === "deselect_all"
                  ? "Deselect All"
                  : f === "mods_only"
                  ? "Mods Only"
                  : "Saves Only"}
              </button>
            ))}
          </div>
          <div className="max-h-48 overflow-y-auto space-y-0.5">
            {plan.actions.map((action, i) => {
              const path = action.SendToRemote?.relative_path
                || action.ReceiveFromRemote?.relative_path
                || action.Conflict?.local.relative_path
                || action.Delete
                || "";
              return (
                <SyncActionItem
                  key={path || i}
                  action={action}
                  excluded={excluded.has(path)}
                  onToggle={toggleExclusion}
                />
              );
            })}
          </div>
        </div>
      )}

      {syncProgress && (
        <div className="mt-3">
          <div className="flex justify-between text-xs text-txt-dim mb-1">
            <span className="truncate mr-3">{syncProgress.file}</span>
            <span className="shrink-0">
              {syncProgress.files_done}/{syncProgress.files_total} files
              {" · "}
              {formatBytes(syncProgress.bytes_sent)} / {formatBytes(syncProgress.bytes_total)}
              {" · "}
              {syncProgress.bytes_total > 0
                ? Math.round((syncProgress.bytes_sent / syncProgress.bytes_total) * 100)
                : 0}%
              {speedText && ` · ${speedText}`}
              {etaText && ` · ETA ${etaText}`}
            </span>
          </div>
          <div className="w-full h-1.5 bg-bg rounded-full overflow-hidden">
            <div
              className="h-full bg-accent rounded-full transition-all"
              style={{
                width: `${syncProgress.bytes_total > 0 ? (syncProgress.bytes_sent / syncProgress.bytes_total) * 100 : 0}%`,
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}
