import { useCallback, useEffect, useMemo, useState } from "react";
import { Copy, Loader2, Trash2, EyeOff, X } from "lucide-react";
import type { DuplicateGroup } from "../lib/types";
import { formatBytes, isDisabledPath } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastError, toastSuccess } from "../lib/toast";
import * as cmd from "../lib/commands";

interface Props {
  gameId: string;
  onClose: () => void;
}

/** Default copy to keep: the first enabled one (groups are sorted by path). */
function defaultKeep(g: DuplicateGroup): string {
  return (g.files.find((f) => !isDisabledPath(f.relative_path)) ?? g.files[0]).relative_path;
}

export default function DuplicateFinder({ gameId, onClose }: Props) {
  const setManifest = useAppStore((s) => s.setManifest);
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [keep, setKeep] = useState<Record<string, string>>({});
  const [confirmDelete, setConfirmDelete] = useState<{ paths: string[]; bytes: number } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      // Hashes the game's files (cached where unchanged) and groups identical ones.
      const g = await cmd.findDuplicates(gameId);
      setGroups(g);
      setKeep(Object.fromEntries(g.map((x) => [x.hash, defaultKeep(x)])));
    } catch (e) {
      toastError(`Duplicate scan failed: ${e}`);
      setGroups([]);
    } finally {
      setLoading(false);
    }
  }, [gameId]);

  useEffect(() => { load(); }, [load]);

  const extras = useCallback(
    (g: DuplicateGroup) => g.files.filter((f) => f.relative_path !== (keep[g.hash] ?? defaultKeep(g))),
    [keep],
  );

  const totalWasted = useMemo(() => (groups ?? []).reduce((n, g) => n + g.wasted, 0), [groups]);

  const refresh = async () => {
    try {
      setManifest(await cmd.scanFiles(gameId));
    } catch { /* ignore */ }
    await load();
  };

  const disable = async (targets: DuplicateGroup[]) => {
    const paths = targets.flatMap(extras).map((f) => f.relative_path).filter((p) => !isDisabledPath(p));
    if (paths.length === 0) return;
    setBusy(true);
    let ok = 0;
    const errors: string[] = [];
    for (const p of paths) {
      try {
        await cmd.toggleMod(p, false);
        ok++;
      } catch (e) {
        errors.push(`${p}: ${e}`);
      }
    }
    setBusy(false);
    if (ok) toastSuccess(`Disabled ${ok} duplicate${ok !== 1 ? "s" : ""}`);
    if (errors.length) toastError(`${errors.length} file(s) couldn't be disabled: ${errors[0]}`);
    await refresh();
  };

  const askDelete = (targets: DuplicateGroup[]) => {
    const files = targets.flatMap(extras);
    if (files.length === 0) return;
    setConfirmDelete({ paths: files.map((f) => f.relative_path), bytes: files.reduce((n, f) => n + f.size, 0) });
  };

  const doDelete = async () => {
    if (!confirmDelete) return;
    setBusy(true);
    try {
      const r = await cmd.deleteModFiles(confirmDelete.paths);
      if (r.deleted) toastSuccess(`Deleted ${r.deleted} duplicate${r.deleted !== 1 ? "s" : ""}`);
      if (r.errors.length) toastError(`${r.errors.length} file(s) couldn't be deleted: ${r.errors[0]}`);
    } catch (e) {
      toastError(`Delete failed: ${e}`);
    } finally {
      setBusy(false);
      setConfirmDelete(null);
    }
    await refresh();
  };

  return (
    <div className="bg-bg-card border border-border rounded-xl p-4 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Copy size={16} className="text-accent-light" />
          <span className="font-medium text-sm">Duplicate files</span>
          {groups && groups.length > 0 && (
            <span className="text-xs text-txt-dim">
              {groups.length} group{groups.length !== 1 ? "s" : ""} · {formatBytes(totalWasted)} wasted
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {groups && groups.length > 0 && !confirmDelete && (
            <>
              <button
                onClick={() => disable(groups)}
                disabled={busy}
                className="flex items-center gap-1 px-2 py-0.5 rounded bg-bg border border-border text-[11px] text-txt-dim hover:border-accent/50 disabled:opacity-50"
              >
                <EyeOff size={11} /> Disable all extras
              </button>
              <button
                onClick={() => askDelete(groups)}
                disabled={busy}
                className="flex items-center gap-1 px-2 py-0.5 rounded bg-status-red/15 text-status-red text-[11px] hover:bg-status-red/25 disabled:opacity-50"
              >
                <Trash2 size={11} /> Delete all extras
              </button>
            </>
          )}
          <button onClick={onClose} className="text-txt-dim hover:text-txt" aria-label="Close duplicate finder">
            <X size={14} />
          </button>
        </div>
      </div>

      {confirmDelete && (
        <div className="flex items-center gap-2 bg-status-red/10 border border-status-red/30 rounded-lg p-2 text-xs">
          <span className="flex-1">
            Permanently delete {confirmDelete.paths.length} file{confirmDelete.paths.length !== 1 ? "s" : ""} ({formatBytes(confirmDelete.bytes)})? The copy marked "Keep" stays.
          </span>
          <button onClick={doDelete} disabled={busy} className="px-2 py-0.5 rounded bg-status-red text-white font-medium disabled:opacity-50">
            {busy ? "Deleting…" : "Delete"}
          </button>
          <button onClick={() => setConfirmDelete(null)} disabled={busy} className="px-2 py-0.5 rounded bg-bg border border-border text-txt-dim">
            Cancel
          </button>
        </div>
      )}

      {loading ? (
        <p className="flex items-center gap-2 text-xs text-txt-dim">
          <Loader2 size={12} className="animate-spin" /> Hashing files to find identical copies…
        </p>
      ) : groups && groups.length === 0 ? (
        <p className="text-xs text-txt-dim">No duplicate files found.</p>
      ) : (
        <div className="space-y-2 max-h-96 overflow-y-auto">
          {(groups ?? []).map((g) => (
            <div key={g.hash} className="bg-bg rounded-lg p-2.5">
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[11px] text-txt-dim">
                  {g.files.length} copies · {formatBytes(g.size)} each · {formatBytes(g.wasted)} wasted
                </span>
                <div className="flex gap-1.5">
                  <button
                    onClick={() => disable([g])}
                    disabled={busy || extras(g).every((f) => isDisabledPath(f.relative_path))}
                    className="px-2 py-0.5 rounded bg-bg-card border border-border text-[10px] text-txt-dim hover:border-accent/50 disabled:opacity-40"
                  >
                    Disable extras
                  </button>
                  <button
                    onClick={() => askDelete([g])}
                    disabled={busy}
                    className="px-2 py-0.5 rounded bg-status-red/15 text-status-red text-[10px] hover:bg-status-red/25 disabled:opacity-40"
                  >
                    Delete extras
                  </button>
                </div>
              </div>
              {g.files.map((f) => {
                const kept = (keep[g.hash] ?? defaultKeep(g)) === f.relative_path;
                return (
                  <label key={f.relative_path} className="flex items-center gap-2 py-0.5 text-xs cursor-pointer">
                    <input
                      type="radio"
                      name={`keep-${g.hash}`}
                      checked={kept}
                      onChange={() => setKeep((k) => ({ ...k, [g.hash]: f.relative_path }))}
                      className="accent-accent"
                    />
                    <span className={`truncate ${kept ? "text-txt" : "text-txt-dim"}`}>{f.relative_path}</span>
                    {isDisabledPath(f.relative_path) && (
                      <span className="px-1.5 rounded-full bg-status-yellow/15 text-status-yellow text-[10px] shrink-0">Disabled</span>
                    )}
                    {kept && <span className="text-[10px] text-accent-light shrink-0">Keep</span>}
                  </label>
                );
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
