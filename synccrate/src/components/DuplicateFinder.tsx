import { useCallback, useEffect, useMemo, useState } from "react";
import { Copy, Loader2, Trash2, EyeOff, X } from "lucide-react";
import type { DuplicateGroup } from "../lib/types";
import { formatBytes, isDisabledPath } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastError, toastSuccess } from "../lib/toast";
import * as cmd from "../lib/commands";
import { Badge, Banner, Button, Panel, cx } from "./ui";

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
  const [confirmDelete, setConfirmDelete] = useState<{ paths: string[]; keep: Record<string, string>; bytes: number } | null>(null);

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
        await cmd.toggleMod(gameId, p, false);
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
    // The backend re-hashes each extra against its kept copy right before
    // deleting, so a stale list can't remove the only remaining copy.
    const keepFor: Record<string, string> = {};
    for (const g of targets) {
      const kept = keep[g.hash] ?? defaultKeep(g);
      for (const f of extras(g)) keepFor[f.relative_path] = kept;
    }
    setConfirmDelete({ paths: files.map((f) => f.relative_path), keep: keepFor, bytes: files.reduce((n, f) => n + f.size, 0) });
  };

  const doDelete = async () => {
    if (!confirmDelete) return;
    setBusy(true);
    try {
      const r = await cmd.deleteModFiles(gameId, confirmDelete.paths, confirmDelete.keep);
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
    <Panel
      label={
        groups && groups.length > 0 ? (
          <><b>// Duplicates</b> &nbsp;{groups.length} group{groups.length !== 1 ? "s" : ""} · {formatBytes(totalWasted)} wasted</>
        ) : (
          <b>// Duplicates</b>
        )
      }
      title="Duplicate files"
      icon={<Copy size={15} className="text-neon" />}
      actions={
        <>
          {groups && groups.length > 0 && !confirmDelete && (
            <>
              <Button size="sm" onClick={() => disable(groups)} disabled={busy} icon={<EyeOff size={12} />}>
                Disable all extras
              </Button>
              <Button size="sm" variant="danger" onClick={() => askDelete(groups)} disabled={busy} icon={<Trash2 size={12} />}>
                Delete all extras
              </Button>
            </>
          )}
          <button onClick={onClose} className="p-1 text-txt-muted hover:text-txt" aria-label="Close duplicate finder">
            <X size={15} />
          </button>
        </>
      }
      bodyClassName="space-y-3"
    >
      {confirmDelete && (
        <Banner
          tone="danger"
          icon={<Trash2 size={14} />}
          title={`Permanently delete ${confirmDelete.paths.length} file${confirmDelete.paths.length !== 1 ? "s" : ""} (${formatBytes(confirmDelete.bytes)})?`}
          actions={
            <>
              <Button size="sm" variant="danger" onClick={doDelete} disabled={busy}>
                {busy ? "Deleting…" : "Delete"}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(null)} disabled={busy}>
                Cancel
              </Button>
            </>
          }
        >
          The copy marked "Keep" stays.
        </Banner>
      )}

      {loading ? (
        <p className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.08em] text-txt-dim">
          <Loader2 size={12} className="animate-spin text-neon" /> Hashing files to find identical copies…
        </p>
      ) : groups && groups.length === 0 ? (
        <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-dim">No duplicate files found.</p>
      ) : (
        <div className="space-y-2 max-h-96 overflow-y-auto pr-1">
          {(groups ?? []).map((g) => (
            <div key={g.hash} className="bg-bg border border-border">
              <div className="flex items-center justify-between gap-3 px-3 py-2 border-b border-border">
                <span className="font-mono text-[11px] text-txt-dim">
                  <span className="text-txt">{g.files.length}</span> copies · {formatBytes(g.size)} each ·{" "}
                  <span className="text-amber">{formatBytes(g.wasted)} wasted</span>
                </span>
                <div className="flex gap-1.5">
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => disable([g])}
                    disabled={busy || extras(g).every((f) => isDisabledPath(f.relative_path))}
                    className="!h-6"
                  >
                    Disable extras
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => askDelete([g])} disabled={busy} className="!h-6 !text-status-red">
                    Delete extras
                  </Button>
                </div>
              </div>
              <div className="px-3 py-1.5">
                {g.files.map((f) => {
                  const kept = (keep[g.hash] ?? defaultKeep(g)) === f.relative_path;
                  return (
                    <label key={f.relative_path} className="flex items-center gap-2.5 py-1 cursor-pointer">
                      <input
                        type="radio"
                        name={`keep-${g.hash}`}
                        checked={kept}
                        onChange={() => setKeep((k) => ({ ...k, [g.hash]: f.relative_path }))}
                        className="accent-neon"
                      />
                      <span className={cx("font-mono text-[11px] truncate", kept ? "text-txt" : "text-txt-muted")}>{f.relative_path}</span>
                      {isDisabledPath(f.relative_path) && <Badge tone="neutral" className="shrink-0">Disabled</Badge>}
                      {kept && <Badge tone="neon" className="shrink-0">Keep</Badge>}
                    </label>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </Panel>
  );
}
