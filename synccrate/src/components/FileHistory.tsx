import { useEffect, useState } from "react";
import { History, Loader2, RotateCcw } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { Button, cx } from "./ui";
import { fileName, formatBytes, formatDate } from "../lib/utils";
import * as cmd from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import type { FileVersion } from "../lib/types";

function describe(v: FileVersion) {
  const from = v.peer ? ` from ${v.peer}` : "";
  if (v.reason === "deleted") return `Deleted by a sync${from}`;
  if (v.reason === "before-restore") return "Before you restored an older version";
  return `Replaced by a sync${from}`;
}

/** Previous versions of synced files (backend `commands::history`): one
 * file's when `path` is given (mod details), otherwise the game's latest. */
export default function FileHistory({ gameId, path, limit, className }: { gameId: string; path?: string; limit?: number; className?: string }) {
  const setManifest = useAppStore((s) => s.setManifest);
  const activeGame = useAppStore((s) => s.activeGame);
  const addLog = useLogStore((s) => s.addLog);
  const [versions, setVersions] = useState<FileVersion[] | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    cmd
      .listFileHistory(gameId, path)
      .then(setVersions)
      .catch(() => setVersions([]));

  useEffect(() => {
    setVersions(null);
    setConfirm(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameId, path]);

  const restore = async (v: FileVersion) => {
    setBusy(true);
    try {
      await cmd.restoreFileVersion(gameId, v.id);
      addLog(`Restored the ${formatDate(v.at)} version of ${v.path}`, "success");
      toastSuccess(`Restored ${fileName(v.path)}. The version it replaced is kept here too.`);
      setConfirm(null);
      if (gameId === activeGame) {
        try {
          setManifest(await cmd.scanFiles(gameId));
        } catch {}
      }
      await load();
    } catch (e) {
      toastError(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  if (versions === null) {
    return <p className={cx("text-xs text-txt-muted flex items-center gap-2", className)}><Loader2 size={12} className="animate-spin" /> Loading history…</p>;
  }
  if (versions.length === 0) {
    return (
      <p className={cx("text-xs text-txt-muted", className)}>
        {path ? "No earlier versions. When a sync replaces or deletes this file, the old one is kept here." : "Nothing yet. When a sync replaces or deletes a file, its old version is kept here."}
      </p>
    );
  }
  const shown = limit ? versions.slice(0, limit) : versions;
  return (
    <div className={className}>
      <ul className="border border-border divide-y divide-border">
        {shown.map((v) => (
          <li key={v.id} className="row-y px-3 py-2 flex items-center gap-3">
            <History size={13} className="shrink-0 text-txt-muted" />
            <div className="flex-1 min-w-0">
              {!path && <p className="text-[13px] text-txt truncate" title={v.path}>{fileName(v.path)}</p>}
              <p className="font-mono text-[10.5px] text-txt-muted truncate">
                {formatDate(v.at)} · {describe(v)} · {formatBytes(v.size)}
              </p>
            </div>
            {confirm === v.id ? (
              <span className="flex gap-1.5 shrink-0">
                <Button size="sm" variant="primary" onClick={() => restore(v)} disabled={busy} icon={busy ? <Loader2 size={12} className="animate-spin" /> : <RotateCcw size={12} />}>
                  Put back
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setConfirm(null)} disabled={busy}>Cancel</Button>
              </span>
            ) : (
              <Button size="sm" variant="ghost" onClick={() => setConfirm(v.id)} icon={<RotateCcw size={12} />}>
                Restore
              </Button>
            )}
          </li>
        ))}
      </ul>
      {limit && versions.length > limit && <p className="text-[11px] text-txt-muted mt-2">…and {versions.length - limit} older versions. Open a mod's details to see all of its versions.</p>}
    </div>
  );
}
