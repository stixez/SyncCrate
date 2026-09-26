import { useEffect, useState } from "react";
import { FolderOpen, Loader2, Trash2 } from "lucide-react";
import * as cmd from "../lib/commands";
import type { StorageUsage } from "../lib/commands";
import { gameLabel } from "../lib/games";
import { formatBytes } from "../lib/utils";
import { toastError, toastSuccess } from "../lib/toast";
import { friendlyError } from "../lib/errors";
import { Button, Panel } from "./ui";

/**
 * How much SyncCrate keeps on disk, per game, with a way to clean up file
 * history (backups are deleted on the Backups page). Nothing showed this.
 */
export default function StoragePanel() {
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  // A failed measurement showed "Measuring…" forever.
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    cmd
      .storageUsage()
      .then((u) => { setUsage(u); setError(null); })
      .catch((e) => setError(friendlyError(e)));
  useEffect(() => { load(); }, []);

  const clear = async (game: string) => {
    setConfirm(null);
    setBusy(true);
    try {
      const n = await cmd.clearFileHistory(game);
      toastSuccess(`Removed ${n} earlier version${n !== 1 ? "s" : ""} of ${gameLabel(game)} files`);
      await load();
    } catch (e) {
      toastError(`Couldn't clear the file history: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel
      title="Storage"
      label="// On this PC"
      actions={
        usage && (
          <Button size="sm" variant="ghost" onClick={() => cmd.openFolder(usage.folder).catch((e) => toastError(`${e}`))} icon={<FolderOpen size={12} />}>
            Open folder
          </Button>
        )
      }
    >
      {error && !usage ? (
        <p className="text-xs text-txt-dim">
          Couldn't measure it: {error}{" "}
          <button className="text-neon hover:underline" onClick={() => { setError(null); load(); }}>Try again</button>
        </p>
      ) : !usage ? (
        <p className="flex items-center gap-2 text-xs text-txt-dim"><Loader2 size={12} className="animate-spin" /> Measuring…</p>
      ) : (
        <div className="space-y-3">
          <p className="text-[13px] text-txt-dim">
            SyncCrate uses <b className="font-medium text-txt">{formatBytes(usage.on_disk)}</b> for backups, file history, game art and settings.
            Files that are the same in several backups are stored once.
          </p>
          {usage.games.length > 0 && (
            <div className="border border-border divide-y divide-border">
              {usage.games.map((g) => (
                <div key={g.game} className="row-y flex items-center gap-3 px-3 py-2">
                  <span className="text-[13px] text-txt flex-1 min-w-0 truncate">{gameLabel(g.game)}</span>
                  <span className="font-mono text-[11px] text-txt-muted tabular shrink-0 whitespace-nowrap">
                    {g.backups} backup{g.backups !== 1 ? "s" : ""} · {formatBytes(g.backup_bytes)}
                  </span>
                  <span className="font-mono text-[11px] text-txt-muted tabular shrink-0 whitespace-nowrap">
                    {g.history_versions} earlier version{g.history_versions !== 1 ? "s" : ""} · {formatBytes(g.history_bytes)}
                  </span>
                  {confirm === g.game ? (
                    <span className="flex gap-1.5 shrink-0">
                      <Button size="sm" variant="danger" onClick={() => clear(g.game)} disabled={busy}>Delete</Button>
                      <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>Cancel</Button>
                    </span>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => setConfirm(g.game)}
                      disabled={busy || g.history_versions === 0}
                      icon={<Trash2 size={12} />}
                      title="Delete the earlier versions file history kept for this game. Undo of the last sync may then not be able to put replaced files back."
                    >
                      Clear history
                    </Button>
                  )}
                </div>
              ))}
            </div>
          )}
          {confirm && (
            <p className="text-[11px] text-amber">
              Deleting {gameLabel(confirm)}'s file history can't be undone, and "Undo last sync" may then not be able to put replaced files back.
            </p>
          )}
        </div>
      )}
    </Panel>
  );
}
