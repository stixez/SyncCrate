import { useEffect, useState } from "react";
import { Loader2, Undo2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import { formatDate } from "../lib/utils";
import type { UndoStatus } from "../lib/types";
import { Banner, Button } from "./ui";

/**
 * "Undo last sync" on the Dashboard. The toast after a sync disappears after a
 * few seconds, and undo was otherwise only on the Backups page.
 */
export default function UndoLastSync({ gameId }: { gameId: string }) {
  const addLog = useLogStore((s) => s.addLog);
  // Set after every sync (any game): refetch this game's own status then.
  const lastSync = useAppStore((s) => s.undoStatus);
  const [status, setStatus] = useState<UndoStatus | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    cmd.getUndoStatus(gameId).then((s) => { if (!cancelled) setStatus(s); }).catch(() => { if (!cancelled) setStatus(null); });
    return () => { cancelled = true; };
  }, [gameId, lastSync]);

  if (!status || (!status.added && !status.replaced && !status.deleted)) return null;

  const undo = async () => {
    setConfirm(false);
    setBusy(true);
    try {
      const r = await cmd.undoLastSync(gameId);
      const parts = [`${r.restored} restored`, `${r.removed} removed`];
      if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
      setStatus(null);
      useAppStore.getState().setUndoStatus(null);
      // "What's new" listed the files the undo just removed.
      useAppStore.getState().setLastSyncChanges(null);
      addLog(`Sync undone: ${parts.join(", ")}`, "success");
      toastSuccess(`Sync undone: ${parts.join(", ")}`);
      cmd.scanFiles(gameId).then((m) => useAppStore.getState().setManifest(m, gameId)).catch(() => {});
    } catch (e) {
      addLog(`Undo failed: ${e}`, "error");
      toastError(`Undo failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const what = [
    status.added && `${status.added} added`,
    status.replaced && `${status.replaced} replaced`,
    status.deleted && `${status.deleted} deleted`,
  ].filter(Boolean).join(", ");

  return (
    <Banner
      tone="info"
      icon={<Undo2 size={14} />}
      title={`Last sync ${formatDate(status.created_at)}: ${what}`}
      actions={
        confirm ? (
          <>
            <Button size="sm" variant="primary" onClick={undo} disabled={busy}>
              Undo it
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(false)} disabled={busy}>
              Cancel
            </Button>
          </>
        ) : (
          <Button size="sm" onClick={() => setConfirm(true)} disabled={busy} icon={busy ? <Loader2 size={12} className="animate-spin" /> : <Undo2 size={12} />}>
            {busy ? "Undoing..." : "Undo last sync"}
          </Button>
        )
      }
    >
      {confirm ? "Your files go back to how they were before it. Files you've changed since are left alone." : null}
    </Banner>
  );
}
