import { useState, useEffect, useMemo } from "react";
import { Archive, Plus, RotateCcw, Trash2, Pencil, Check, X, Loader2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { formatBytes, formatDate } from "../lib/utils";
import { gameLabel } from "../lib/games";
import { Badge, Banner, Button, EmptyState, Input, Panel, SectionHeader, cx } from "./ui";
import * as cmd from "../lib/commands";
import { toastError, toastInfo } from "../lib/toast";

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

export default function BackupList({ gameId }: Props) {
  const backups = useAppStore((s) => s.backups);
  const setBackups = useAppStore((s) => s.setBackups);
  const addLog = useLogStore((s) => s.addLog);

  const [label, setLabel] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");

  useEffect(() => {
    cmd.listBackups().then(setBackups).catch(console.error);
  }, [setBackups]);

  // Filter backups to current game
  const filteredBackups = useMemo(() => {
    return backups.filter((b) => b.game === gameId);
  }, [backups, gameId]);

  const handleCreate = async () => {
    if (!label.trim()) return;
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
    } finally {
      setCreating(false);
    }
  };

  const handleRestore = async (id: string) => {
    if (restoreConfirm !== id) {
      setRestoreConfirm(id);
      return;
    }
    setRestoreConfirm(null);
    setRestoring(true);
    try {
      const r = await cmd.restoreBackup(id);
      const updated = await cmd.listBackups();
      setBackups(updated);
      try {
        const m = await cmd.scanFiles(gameId);
        useAppStore.getState().setManifest(m);
      } catch {}
      addLog(`Backup restored: ${r.restored} file(s) (safety backup created)`, "success");
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

  return (
    <div className="space-y-5">
      <SectionHeader
        label={<><b>// Snapshots</b> &nbsp;{filteredBackups.length} stored</>}
        title="Backups"
        description="Point-in-time copies of every content folder. Restoring always takes a safety backup first."
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
              <Button variant="primary" onClick={handleCreate} disabled={creating || !label.trim()} icon={creating ? <Loader2 size={14} className="animate-spin" /> : <Archive size={14} />}>
                {creating ? "Creating..." : "Create Backup"}
              </Button>
              <Button variant="ghost" onClick={() => setShowCreate(false)}>
                Cancel
              </Button>
            </div>
          </div>
        </Panel>
      )}

      {restoring && (
        <Banner tone="info" icon={<Loader2 size={15} className="animate-spin" />} title="Restoring backup...">
          A safety backup is being created first.
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
            const cats = Object.entries(
              backup.category_counts ?? {
                mods: backup.mods_count ?? 0,
                saves: backup.saves_count ?? 0,
                tray: backup.tray_count ?? 0,
                screenshots: backup.screenshots_count ?? 0,
              },
            ).filter(([, count]) => count > 0);
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
                  <span className={cx("w-[7px] h-[7px]", idx === 0 ? "bg-neon" : backup.auto ? "bg-transparent" : "bg-txt-muted")} />
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
                        {backup.auto && <Badge tone="neutral" className="shrink-0">Auto</Badge>}
                        <Badge tone="neutral" className="shrink-0">{gameLabel(backup.game)}</Badge>
                      </div>
                    </div>
                    <div className="flex gap-2 shrink-0">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => handleRestore(backup.id)}
                        disabled={restoring}
                        icon={<RotateCcw size={12} />}
                        className={restorePending ? "!text-amber [--btn-line:rgb(var(--color-amber))] [--btn-fill:rgb(var(--color-amber)/0.1)]" : undefined}
                      >
                        {restorePending ? "Confirm Restore?" : "Restore"}
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
                    {cats.map(([cat, count]) => (
                      <span key={cat} className="uppercase tracking-[0.06em]">
                        <span className="text-txt-dim tabular">{count}</span> {cat}
                      </span>
                    ))}
                  </div>
                  {restorePending && (
                    <p className="mt-2 text-[11px] text-amber">Replaces current files. A safety backup will be created first.</p>
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
