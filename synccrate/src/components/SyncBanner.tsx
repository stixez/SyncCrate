import { useRef, useEffect, useState, useMemo, useCallback, type ReactNode } from "react";
import { ArrowUpDown, ArrowUp, ArrowDown, AlertTriangle, ChevronDown, ChevronUp, Gamepad2, X } from "lucide-react";
import type { SyncPlan } from "../lib/types";
import { formatBytes } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { gameLabel } from "../lib/games";
import SyncActionItem from "./SyncActionItem";
import * as cmd from "../lib/commands";
import { toastError } from "../lib/toast";
import { Banner, Button, LiveDot, ProgressBar, cx } from "./ui";

interface SyncBannerProps {
  plan: SyncPlan;
  onSync: () => void;
  /** A sync was started (progress may not have arrived yet). */
  busy?: boolean;
  onResolveAll?: (strategy: string) => void;
}

function formatEta(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "--";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.ceil(seconds % 60);
  return `${m}m ${s}s`;
}

/** Rough pre-sync duration: "about 30 s" / "about 4 min". */
function formatEstimate(seconds: number): string {
  if (seconds < 60) return `about ${Math.max(5, Math.round(seconds / 5) * 5)} s`;
  if (seconds < 3600) return `about ${Math.round(seconds / 60)} min`;
  const h = seconds / 3600;
  return `about ${h < 10 ? h.toFixed(1).replace(/\.0$/, "") : Math.round(h)} h`;
}

export default function SyncBanner({ plan, onSync, onResolveAll, busy }: SyncBannerProps) {
  const syncProgress = useAppStore((s) => s.syncProgress);
  const backupProgress = useAppStore((s) => s.backupProgress);
  const preparing = !!busy && !syncProgress;
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const session = useAppStore((s) => s.session);
  const startTimeRef = useRef<number | null>(null);
  const startBytesRef = useRef<number>(0);
  const [expanded, setExpanded] = useState(false);
  const [quickFilter, setQuickFilter] = useState<string | null>(null);
  const [visibleCount, setVisibleCount] = useState(50);
  const activeGame = useAppStore((s) => s.activeGame);
  const [gameRunning, setGameRunning] = useState(false);
  const [typicalSpeed, setTypicalSpeed] = useState<number | null>(null);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    if (!syncProgress) setCancelling(false);
  }, [syncProgress]);

  const handleCancel = async () => {
    setCancelling(true);
    try {
      const wasSyncing = await cmd.cancelSync();
      if (!wasSyncing) setCancelling(false);
    } catch {
      setCancelling(false);
    }
  };

  // Warn (don't block) while the game is running: files may be locked or half-loaded.
  useEffect(() => {
    let cancelled = false;
    const check = () =>
      cmd.checkGameRunning(activeGame)
        .then((running) => { if (!cancelled) setGameRunning(running); })
        .catch(() => {});
    check();
    const timer = setInterval(check, 5000);
    return () => { cancelled = true; clearInterval(timer); };
  }, [activeGame]);

  useEffect(() => {
    cmd.getTypicalTransferSpeed().then(setTypicalSpeed).catch(() => {});
  }, [plan]);

  // Reset visible count when plan changes or details expand
  useEffect(() => {
    setVisibleCount(50);
  }, [plan, expanded]);

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
        toastError(`Couldn't change the selection: ${e}`);
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
        toastError(`Couldn't apply the filter: ${e}`);
      }
    },
    [plan, session, setSyncPlan],
  );

  const excludedCount = excluded.size;

  const pct = syncProgress && syncProgress.bytes_total > 0
    ? (syncProgress.bytes_sent / syncProgress.bytes_total) * 100
    : 0;
  const blocked = conflictCount > 0;
  const state = syncProgress ? "Syncing" : preparing ? "Preparing" : blocked ? "Blocked" : "Ready";

  const QUICK_FILTERS: { id: string; label: string }[] = [
    { id: "select_all", label: "Select all" },
    { id: "deselect_all", label: "Deselect all" },
    { id: "mods_only", label: "Mods only" },
    { id: "saves_only", label: "Saves only" },
  ];

  return (
    <div className={cx("panel", blocked && !syncProgress ? "panel-warn" : "panel-accent")}>
      {/* Header: state + primary actions */}
      <div className="px-5 pt-4 pb-4 flex items-start justify-between gap-4 flex-wrap">
        <div className="min-w-0">
          <p className="hud-label mb-1.5 flex items-center gap-2">
            {syncProgress ? <LiveDot /> : blocked ? <LiveDot tone="amber" /> : <ArrowUpDown size={12} className="text-neon" />}
            <span>// Sync plan</span>
            <span className={blocked && !syncProgress ? "text-amber" : "text-neon"}>{state}</span>
            {excludedCount > 0 && <span className="text-txt-muted">· {excludedCount} excluded</span>}
          </p>
          <h3 className="display text-[1.5rem] text-txt">
            {syncProgress ? (
              <>Syncing <span className="text-neon">{Math.round(pct)}%</span></>
            ) : preparing ? (
              <>Getting <span className="text-neon">ready</span></>
            ) : blocked ? (
              <>{conflictCount} conflict{conflictCount !== 1 ? "s" : ""} <span className="text-amber">to resolve</span></>
            ) : (
              <>Ready to <span className="text-neon">sync</span></>
            )}
          </h3>
          {preparing && (
            <p className="text-xs text-txt-dim mt-2">
              {backupProgress?.phase === "presync" && backupProgress.files_total > 0
                ? `Backing up the files this sync replaces: ${backupProgress.files_done} of ${backupProgress.files_total}`
                : "Saving copies of the files this sync changes, then downloading."}
            </p>
          )}
          {conflictCount > 0 && !preparing && (
            <p className="text-xs text-txt-dim mt-2">
              {conflictCount} conflict{conflictCount !== 1 ? "s" : ""} must be resolved before syncing
              {modConflicts > 0 && saveConflicts > 0 && ` (${modConflicts} mod, ${saveConflicts} save)`}
            </p>
          )}
        </div>
        {conflictCount === 0 ? (
          <Button
            variant="primary"
            size="lg"
            onClick={onSync}
            disabled={!!syncProgress || busy}
            icon={<ArrowUpDown size={15} />}
          >
            {syncProgress ? "Syncing..." : busy ? "Preparing..." : "Sync Now"}
          </Button>
        ) : (
          <div className="flex items-center gap-2 flex-wrap">
            {onResolveAll && (
              <Button
                variant="primary"
                onClick={() => onResolveAll("use_newest")}
                disabled={!!syncProgress}
                title="Resolve every conflict by keeping whichever copy was modified more recently (ties keep yours)"
              >
                Keep newer for all
              </Button>
            )}
            <Button onClick={() => setPage("content")}>View Conflicts</Button>
          </div>
        )}
      </div>

      {/* Telemetry strip */}
      <div className="grid grid-cols-4 border-y border-border bg-bg/60">
        <Readout label="Upload" value={sendCount} unit="files" icon={<ArrowUp size={11} />} />
        <Readout label="Download" value={receiveCount} unit="files" icon={<ArrowDown size={11} />} />
        <Readout
          label="Conflicts"
          value={conflictCount}
          unit={modConflicts > 0 && saveConflicts > 0 ? `${modConflicts} mod · ${saveConflicts} save` : "files"}
          icon={<AlertTriangle size={11} />}
          warn={conflictCount > 0}
        />
        <Readout
          label="Total"
          value={formatBytes(plan.total_bytes)}
          unit={
            typicalSpeed && plan.total_bytes > 0 && !syncProgress
              ? formatEstimate(plan.total_bytes / typicalSpeed)
              : "to transfer"
          }
          last
        />
      </div>

      <div className="px-5 py-3 space-y-2">
        {gameRunning && (
          <Banner tone="warn" icon={<Gamepad2 size={14} />}>
            {gameLabel(activeGame)} is running — close it before syncing so files aren't locked or half-loaded.
          </Banner>
        )}

        {plan.warning && (
          <Banner tone="warn" icon={<AlertTriangle size={14} />}>
            {plan.warning}
          </Banner>
        )}
        {!plan.warning && (plan.skipped_foreign ?? 0) > 0 && (
          <p className="text-xs text-txt-dim">
            {plan.skipped_foreign} host file{plan.skipped_foreign !== 1 ? "s" : ""} outside {gameLabel(activeGame)}'s folders{" "}
            {plan.skipped_foreign !== 1 ? "were" : "was"} skipped.
          </p>
        )}
        {(plan.pack_unavailable?.length ?? 0) > 0 && (
          <p className="text-xs text-txt-dim">
            {plan.pack_unavailable!.length} pack file{plan.pack_unavailable!.length !== 1 ? "s" : ""} this host doesn't have — try someone else who has them.
          </p>
        )}
        {((plan.disabled_locally ?? 0) > 0 || (plan.disabled_on_host ?? 0) > 0) && (
          <p className="text-xs text-txt-dim">
            {[
              (plan.disabled_locally ?? 0) > 0 &&
                `${plan.disabled_locally} mod${plan.disabled_locally !== 1 ? "s" : ""} you disabled already match the host and stay disabled`,
              (plan.disabled_on_host ?? 0) > 0 &&
                `${plan.disabled_on_host} mod${plan.disabled_on_host !== 1 ? "s" : ""} the host disabled ${plan.disabled_on_host !== 1 ? "were" : "was"} left as you have ${plan.disabled_on_host !== 1 ? "them" : "it"}`,
            ].filter(Boolean).join(" · ")}
            .
          </p>
        )}

        {syncProgress && (
          <div className="pt-1">
            <div className="flex items-end justify-between gap-4 mb-2">
              <div className="min-w-0">
                <p className="hud-label mb-0.5">Now transferring</p>
                <p className="font-mono text-xs text-txt truncate" title={syncProgress.file}>{syncProgress.file}</p>
              </div>
              <Button
                variant="danger"
                size="sm"
                onClick={handleCancel}
                disabled={cancelling}
                title="Stop after the current file. The next sync resumes where it stopped."
                icon={<X size={12} />}
              >
                {cancelling ? "Cancelling…" : "Cancel"}
              </Button>
            </div>
            <ProgressBar value={pct} />
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mt-2 font-mono text-[11px] text-txt-dim tabular">
              <span><span className="text-txt">{syncProgress.files_done}</span>/{syncProgress.files_total} files</span>
              <span>
                <span className="text-txt">{formatBytes(syncProgress.bytes_sent)}</span> / {formatBytes(syncProgress.bytes_total)}
              </span>
              <span className="text-neon">{Math.round(pct)}%</span>
              {speedText && <span>{speedText}</span>}
              {etaText && <span>ETA <span className="text-txt">{etaText}</span></span>}
            </div>
          </div>
        )}

        <button
          onClick={() => setExpanded(!expanded)}
          aria-expanded={expanded}
          className="flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-[0.1em] text-txt-muted hover:text-neon transition-colors"
        >
          {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          {expanded ? "Hide files" : `Files in this sync (${plan.actions.length})`}
        </button>

        {expanded && (
          <div className="pt-1 pb-1">
            <div className="flex mb-2 flex-wrap">
              {QUICK_FILTERS.map((f, i) => (
                <button
                  key={f.id}
                  onClick={() => applyQuickFilter(f.id)}
                  className={cx(
                    "h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border transition-colors",
                    i > 0 && "-ml-px",
                    quickFilter === f.id
                      ? "relative z-[1] bg-neon/10 border-neon text-neon"
                      : "bg-bg border-line-hi text-txt-dim hover:text-txt",
                  )}
                >
                  {f.label}
                </button>
              ))}
            </div>
            <div className="max-h-48 overflow-y-auto border border-border bg-bg divide-y divide-border/60">
              {plan.actions.slice(0, visibleCount).map((action, i) => {
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
              {plan.actions.length > visibleCount && (
                <button
                  onClick={() => setVisibleCount((c) => c + 50)}
                  className="w-full text-center py-2 font-mono text-[11px] uppercase tracking-[0.08em] text-txt-dim hover:text-neon transition-colors"
                >
                  Show more ({plan.actions.length - visibleCount} remaining)
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function Readout({
  label,
  value,
  unit,
  icon,
  warn,
  last,
}: {
  label: string;
  value: ReactNode;
  unit: string;
  icon?: ReactNode;
  warn?: boolean;
  last?: boolean;
}) {
  return (
    <div className={cx("px-5 py-3 min-w-0", !last && "border-r border-border")}>
      <p className={cx("hud-label flex items-center gap-1.5", warn && "!text-amber")}>
        {icon}
        {label}
      </p>
      <p className={cx("font-display font-bold text-[1.5rem] leading-none tabular mt-1.5 truncate", warn ? "text-amber" : "text-txt")}>
        {value}
      </p>
      <p className="font-mono text-[10px] text-txt-muted mt-1 truncate">{unit}</p>
    </div>
  );
}
