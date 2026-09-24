import { useState, useEffect, useMemo } from "react";
import { Archive, Plus, RotateCcw, Trash2, Pencil, Check, X, Loader2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { formatBytes, formatDate } from "../lib/utils";
import { gameLabel, getGameDef } from "../lib/games";
import { Badge, Banner, Button, EmptyState, Input, Panel, ProgressBar, SectionHeader, Toggle, cx } from "./ui";
import * as cmd from "../lib/commands";
import { toastError, toastInfo, toastSuccess } from "../lib/toast";
import type { BackupInfo, BackupProgress, RestoreResult } from "../lib/types";

function sendNotification(title: string, body: string) {
  try {
    if ("Notification" in window && Notification.permission === "granted") {
      new Notification(title, { body });
    }
  } catch {
    // Notifications not supported
  }
}

interface Props {
  gameId: string;
}

const KIND_LABEL: Record<string, string> = {
  auto: "Scheduled",
  presync: "Before sync",
  safety: "Safety",
};

const PHASE_LABEL: Record<BackupProgress["phase"], string> = {
  manual: "Backing up",
  auto: "Scheduled backup",
  presync: "Backing up files the sync replaces",
  safety: "Safety backup of current files",
  restore: "Restoring files",
};

/** Per-content-type counts with registry labels; old backups only have the Sims-shaped fields. */
function categoryCounts(backup: BackupInfo): [string, number][] {
  const counts =
    backup.category_counts && Object.keys(backup.category_counts).length > 0
      ? backup.category_counts
      : {
          mods: backup.mods_count ?? 0,
          saves: backup.saves_count ?? 0,
          tray: backup.tray_count ?? 0,
          screenshots: backup.screenshots_count ?? 0,
        };
  const types = getGameDef(backup.game)?.content_types ?? [];
  return Object.entries(counts)
    .filter(([, count]) => count > 0)
    .map(([id, count]): [string, number] => [types.find((t) => t.id === id)?.label ?? id, count]);
}

function restoreSummary(r: RestoreResult): string {
  const parts = [`${r.restored} restored`];
  if (r.unchanged) parts.push(`${r.unchanged} already up to date`);
  if (r.removed) parts.push(`${r.removed} removed`);
  if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
  if (r.missing) parts.push(`${r.missing} missing from the backup`);
  return parts.join(", ");
}

export default function BackupList({ gameId }: Props) {
  const backups = useAppStore((s) => s.backups);
  const setBackups = useAppStore((s) => s.setBackups);
  const progress = useAppStore((s) => s.backupProgress);
  const setProgress = useAppStore((s) => s.setBackupProgress);
  const addLog = useLogStore((s) => s.addLog);

  const [label, setLabel] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<string | null>(null);
  const [exactRestore, setExactRestore] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");

  useEffect(() => {
    cmd.listBackups().then(setBackups).catch(console.error);
  }, [setBackups]);

  // Filter backups to current game
  const filteredBackups = useMemo(() => {
    return backups.filter((b) => b.game === gameId);
  }, [backups, gameId]);

  const busy = creating || restoring;

  const handleCreate = async () => {
    if (!label.trim()) return;
    setProgress(null);
    setCreating(true);
    try {
      await cmd.createBackup(label.trim(), gameId);
      const updated = await cmd.listBackups();
      setBackups(updated);
      setLabel("");
      setShowCreate(false);
      addLog(`Backup "${label.trim()}" created`, "success");
      sendNotification("SyncCrate", `Backup "${label.trim()}" created successfully`);
    } catch (e) {
      addLog(`Backup failed: ${e}`, "error");
      toastError(`Backup failed: ${e}`);
    } finally {
      setCreating(false);
      setProgress(null);
    }
  };

  const askRestore = (id: string) => {
    setExactRestore(false);
    setRestoreConfirm(id);
  };

  const handleRestore = async (id: string, exact: boolean) => {
    setRestoreConfirm(null);
    setProgress(null);
    setRestoring(true);
    try {
      const r = await cmd.restoreBackup(id, exact);
      const updated = await cmd.listBackups();
      setBackups(updated);
      try {
        const m = await cmd.scanFiles(gameId);
        useAppStore.getState().setManifest(m);
      } catch {}
      const safety = r.safety_backup ? ` Your previous files are in the safety backup "${r.safety_backup}".` : "";
      if (r.error) {
        // Stopped part-way: say what was written and where the old files are.
        const msg = `Restore stopped: ${r.error}. ${restoreSummary(r)} before it stopped.${safety}`;
        addLog(msg, "error");
        toastError(msg);
        return;
      }
      addLog(`Backup restored: ${restoreSummary(r)}.${safety}`, "success");
      toastSuccess(`Backup restored: ${restoreSummary(r)}`);
      if (r.skipped.length > 0) {
        // Mods the user has since disabled/enabled are left as they are.
        const msg = `${r.skipped.length} file(s) skipped because you've disabled or enabled them since: ${r.skipped[0]}${r.skipped.length > 1 ? " …" : ""}`;
        addLog(msg, "warning");
        toastInfo(msg);
      }
      sendNotification("SyncCrate", "Backup restored successfully");
    } catch (e) {
      addLog(`Restore failed: ${e}`, "error");
      toastError(`Restore failed: ${e}`);
    } finally {
      setRestoring(false);
      setProgress(null);
    }
  };

  const handleDelete = async (id: string) => {
    if (deleteConfirm !== id) {
      setDeleteConfirm(id);
      return;
    }
    setDeleteConfirm(null);
    try {
      await cmd.deleteBackup(id);
      const updated = await cmd.listBackups();
      setBackups(updated);
      addLog("Backup deleted", "info");
    } catch (e) {
      addLog(`Delete failed: ${e}`, "error");
    }
  };

  const handleRename = async (id: string) => {
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === backups.find((b) => b.id === id)?.label) {
      setRenaming(null);
      return;
    }
    try {
      await cmd.renameBackup(id, trimmed);
      const updated = await cmd.listBackups();
      setBackups(updated);
      addLog(`Backup renamed to "${trimmed}"`, "info");
    } catch (e) {
      addLog(`Rename failed: ${e}`, "error");
    }
    setRenaming(null);
  };

  const showProgress = progress && progress.game === gameId && progress.files_total > 0;

  return (
    <div className="space-y-5">
      <SectionHeader
        label={<><b>// Snapshots</b> &nbsp;{filteredBackups.length} stored</>}
        title="Backups"
        description="Point-in-time snapshots of every content folder. Unchanged files are stored only once, so extra backups are cheap. Restoring always takes a safety backup first."
        actions={
          !showCreate && (
            <Button variant="primary" size="sm" onClick={() => setShowCreate(true)} icon={<Plus size={13} />}>
              Create Backup
            </Button>
          )
        }
      />

      {showCreate && (
        <Panel tone="accent" label={<b>// New snapshot</b>} title="New Backup">
          <div className="space-y-3">
            <Input
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !creating) handleCreate(); }}
              maxLength={128}
              placeholder="Backup label (e.g. Before installing new mods)..."
              aria-label="Backup label"
              autoFocus
            />
            <p className="text-xs text-txt-dim">This will back up all content folders for the current game.</p>
            <div className="flex gap-2">
              <Button variant="primary" onClick={handleCreate} disabled={busy || !label.trim()} icon={creating ? <Loader2 size={14} className="animate-spin" /> : <Archive size={14} />}>
                {creating ? "Creating..." : "Create Backup"}
              </Button>
              <Button variant="ghost" onClick={() => setShowCreate(false)}>
                Cancel
              </Button>
            </div>
          </div>
        </Panel>
      )}

      {busy && (
        <Banner tone="info" icon={<Loader2 size={15} className="animate-spin" />} title={restoring ? "Restoring backup..." : "Creating backup..."}>
          {showProgress ? (
            <ProgressBar
              className="mt-1"
              value={(progress.files_done / progress.files_total) * 100}
              label={PHASE_LABEL[progress.phase] ?? progress.phase}
              meta={`${progress.files_done}/${progress.files_total}`}
            />
          ) : restoring ? (
            "A safety backup of your current files is created first."
          ) : (
            "Scanning content folders..."
          )}
        </Banner>
      )}

      {filteredBackups.length === 0 ? (
        <EmptyState
          icon={<Archive size={18} />}
          label="// No snapshots"
          title="No backups yet"
          description="Create a backup to save the current state of your game files."
          action={
            !showCreate && (
              <Button size="sm" variant="primary" onClick={() => setShowCreate(true)} icon={<Plus size={13} />}>
                Create Backup
              </Button>
            )
          }
        />
      ) : (
        // Timeline: a hairline spine with a square node per snapshot; the newest node is neon.
        <ol className="relative pl-7 before:absolute before:left-[7px] before:top-2 before:bottom-2 before:w-px before:bg-line-hi">
          {filteredBackups.map((backup, idx) => {
            const cats = categoryCounts(backup);
            const kind = backup.kind ?? (backup.auto ? "auto" : "manual");
            const presync = kind === "presync";
            const exact = exactRestore && !presync;
            const restorePending = restoreConfirm === backup.id;
            const deletePending = deleteConfirm === backup.id;
            return (
              <li key={backup.id} className="relative pb-3 last:pb-0">
                <span
                  className={cx(
                    "absolute -left-7 top-[18px] w-[15px] h-[15px] grid place-items-center border bg-bg",
                    idx === 0 ? "border-neon" : "border-line-hi",
                  )}
                >
                  <span className={cx("w-[7px] h-[7px]", idx === 0 ? "bg-neon" : kind !== "manual" ? "bg-transparent" : "bg-txt-muted")} />
                </span>
                <div
                  className={cx(
                    "box px-4 py-3 transition-colors",
                    restorePending ? "border-amber/60" : deletePending ? "border-status-red/60" : "hover:border-line-hi",
                  )}
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0">
                      <p className="hud-label">
                        {idx === 0 ? <b>// Latest</b> : "//"} &nbsp;{formatDate(backup.created_at)}
                      </p>
                      <div className="flex items-center gap-2 mt-1 min-w-0">
                        {renaming === backup.id ? (
                          <div className="flex items-center gap-1">
                            <input
                              type="text"
                              value={renameValue}
                              onChange={(e) => setRenameValue(e.target.value)}
                              maxLength={128}
                              className="input input-sm w-64"
                              autoFocus
                              onKeyDown={(e) => {
                                if (e.key === "Enter") handleRename(backup.id);
                                if (e.key === "Escape") setRenaming(null);
                              }}
                            />
                            <button onClick={() => handleRename(backup.id)} className="p-1 text-neon hover:brightness-125" aria-label="Save name">
                              <Check size={14} />
                            </button>
                            <button onClick={() => setRenaming(null)} className="p-1 text-txt-muted hover:text-txt" aria-label="Cancel rename">
                              <X size={14} />
                            </button>
                          </div>
                        ) : (
                          <>
                            <h3 className="font-display font-semibold uppercase tracking-[0.04em] text-[15px] leading-tight truncate">{backup.label}</h3>
                            <button
                              onClick={() => {
                                setRenaming(backup.id);
                                setRenameValue(backup.label);
                              }}
                              className="p-0.5 text-txt-muted hover:text-txt transition-colors shrink-0"
                              title="Rename backup"
                            >
                              <Pencil size={12} />
                            </button>
                          </>
                        )}
                        {KIND_LABEL[kind] && (
                          <Badge tone={kind === "safety" ? "amber" : "neutral"} className="shrink-0">
                            {KIND_LABEL[kind]}
                          </Badge>
                        )}
                        <Badge tone="neutral" className="shrink-0">{gameLabel(backup.game)}</Badge>
                      </div>
                    </div>
                    <div className="flex gap-2 shrink-0">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => (restorePending ? setRestoreConfirm(null) : askRestore(backup.id))}
                        disabled={busy}
                        icon={<RotateCcw size={12} />}
                        className={restorePending ? "!text-amber [--btn-line:rgb(var(--color-amber))] [--btn-fill:rgb(var(--color-amber)/0.1)]" : undefined}
                      >
                        {restorePending ? "Cancel" : "Restore"}
                      </Button>
                      <Button
                        size="sm"
                        variant={deletePending ? "danger" : "ghost"}
                        onClick={() => handleDelete(backup.id)}
                        icon={<Trash2 size={12} />}
                      >
                        {deletePending ? "Confirm?" : "Delete"}
                      </Button>
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mt-2.5 font-mono text-[11px] text-txt-muted">
                    <span><span className="text-txt tabular">{backup.file_count}</span> files</span>
                    <span className="text-txt-dim tabular">{formatBytes(backup.total_size)}</span>
                    {backup.new_bytes !== undefined && backup.new_bytes < backup.total_size && (
                      <span title="Unchanged files are shared with other backups. This is what this backup added on disk.">
                        new data: <span className="text-txt-dim tabular">{formatBytes(backup.new_bytes)}</span>
                      </span>
                    )}
                    {cats.map(([cat, count]) => (
                      <span key={cat} className="uppercase tracking-[0.06em]">
                        <span className="text-txt-dim tabular">{count}</span> {cat}
                      </span>
                    ))}
                  </div>
                  {restorePending && (
                    <div className="mt-3 pt-3 border-t border-line space-y-3">
                      <p className="text-[12px] text-amber">
                        {presync
                          ? "Puts back the files this sync replaced or deleted."
                          : "Replaces your current files with the ones in this backup."}{" "}
                        A safety backup of your current files is created first. Close the game before restoring.
                      </p>
                      <Toggle
                        kind="check"
                        checked={exact}
                        disabled={presync}
                        onChange={setExactRestore}
                        label="Exact restore"
                        description={
                          presync
                            ? "Not available: this backup only holds the files the sync replaced."
                            : "Also remove files added since this backup (only in the backed-up content folders, and only files SyncCrate manages). They stay in the safety backup."
                        }
                      />
                      <div className="flex gap-2">
                        <Button size="sm" variant="primary" onClick={() => handleRestore(backup.id, exact)} disabled={busy} icon={<RotateCcw size={12} />}>
                          {exact ? "Restore exactly" : "Restore"}
                        </Button>
                        <Button size="sm" variant="ghost" onClick={() => setRestoreConfirm(null)}>
                          Cancel
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}
