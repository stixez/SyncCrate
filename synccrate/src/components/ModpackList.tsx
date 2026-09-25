import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowUpDown, Copy, Link2, Loader2, Save as SaveIcon, Share2, Upload, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { Banner, Button, EmptyState, Input, Panel, SectionHeader, StatTile, Toggle, cx } from "./ui";
import { getGameDef, gameLabel } from "../lib/games";
import { formatBytes, formatDate } from "../lib/utils";
import * as cmd from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import type { ModPack, PackComparison, PackFileStatus } from "../lib/types";

interface Props {
  gameId: string;
}

function groupByContentType(entries: PackFileStatus[]): [string, PackFileStatus[]][] {
  const groups = new Map<string, PackFileStatus[]>();
  for (const e of entries) {
    const key = e.content_type ?? "other";
    groups.set(key, [...(groups.get(key) ?? []), e]);
  }
  return Array.from(groups.entries());
}

export default function ModpackList({ gameId }: Props) {
  const session = useAppStore((s) => s.session);
  const addLog = useLogStore((s) => s.addLog);
  const setPage = useAppStore((s) => s.setPage);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const pendingImportPackPath = useAppStore((s) => s.pendingImportPackPath);
  const setPendingImportPackPath = useAppStore((s) => s.setPendingImportPackPath);
  const pendingImportPack = useAppStore((s) => s.pendingImportPack);
  const setPendingImportPack = useAppStore((s) => s.setPendingImportPack);

  const def = getGameDef(gameId);
  const contentTypes = def?.content_types ?? [];

  // Export
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [selectedTypes, setSelectedTypes] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState(false);
  const [exportedPack, setExportedPack] = useState<ModPack | null>(null);

  // Import
  const [importedPack, setImportedPack] = useState<ModPack | null>(null);
  const [comparison, setComparison] = useState<PackComparison | null>(null);
  const [linkInput, setLinkInput] = useState("");
  const [comparing, setComparing] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [gettingFiles, setGettingFiles] = useState(false);

  const runComparison = async (pack: ModPack) => {
    setImportedPack(pack);
    setComparison(null);
    setComparing(true);
    try {
      const cmp = await cmd.comparePack(pack);
      setComparison(cmp);
      if (!cmp.wrong_game) {
        addLog(
          `Pack "${cmp.pack_name}" compared: ${cmp.have} have, ${cmp.missing.length} missing, ${cmp.different.length} different`,
          "info",
        );
      }
    } catch (e) {
      toastError(`Couldn't read pack: ${e}`);
      setImportedPack(null);
    } finally {
      setComparing(false);
    }
  };

  // A `.scpack` dropped anywhere in the app (App.tsx's global drop handler).
  useEffect(() => {
    if (!pendingImportPackPath) return;
    const path = pendingImportPackPath;
    setPendingImportPackPath(null);
    cmd
      .loadPackFile(path)
      .then(runComparison)
      .catch((e) => toastError(`Couldn't open pack: ${e}`));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingImportPackPath]);

  // A clicked pack link or double-clicked `.scpack` (already validated by
  // the backend, see useOpenIntents). Comparing only reads the local folder.
  useEffect(() => {
    if (!pendingImportPack) return;
    const pack = pendingImportPack;
    setPendingImportPack(null);
    runComparison(pack);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingImportPack]);

  const toggleType = (id: string) => {
    setSelectedTypes((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleCreate = async () => {
    if (!name.trim()) return;
    setCreating(true);
    setExportedPack(null);
    try {
      const pack = await cmd.createPack(
        name.trim(),
        description.trim(),
        gameId,
        selectedTypes.size > 0 ? Array.from(selectedTypes) : undefined,
        undefined,
        true,
      );
      setExportedPack(pack);
      addLog(`Pack "${name.trim()}" built: ${pack.files.length} files`, "success");
    } catch (e) {
      toastError(`Couldn't build pack: ${e}`);
      addLog(`Pack build failed: ${e}`, "error");
    } finally {
      setCreating(false);
    }
  };

  const handleSaveFile = async () => {
    if (!exportedPack) return;
    try {
      const dest = await save({
        defaultPath: `${exportedPack.name}.scpack`,
        filters: [{ name: "SyncCrate Modpack", extensions: ["scpack"] }],
      });
      if (dest) {
        await cmd.savePack(exportedPack, dest);
        addLog(`Pack saved as ${dest.split(/[/\\]/).pop()}`, "success");
        toastSuccess("Pack saved");
      }
    } catch (e) {
      toastError(`Couldn't save pack: ${e}`);
    }
  };

  const handleCopyLink = async () => {
    if (!exportedPack) return;
    try {
      const link = await cmd.packToLink(exportedPack);
      await navigator.clipboard.writeText(link);
      toastSuccess("Link copied to clipboard");
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleImportFile = async () => {
    try {
      const selected = await open({ multiple: false, filters: [{ name: "SyncCrate Modpack", extensions: ["scpack"] }] });
      if (selected && typeof selected === "string") {
        const pack = await cmd.loadPackFile(selected);
        await runComparison(pack);
      }
    } catch (e) {
      toastError(`Couldn't open pack: ${e}`);
    }
  };

  const handleImportLink = async () => {
    if (!linkInput.trim()) return;
    try {
      const pack = await cmd.loadPackLink(linkInput.trim());
      setLinkInput("");
      await runComparison(pack);
    } catch (e) {
      toastError(`Couldn't read that link: ${e}`);
    }
  };

  const handleSwitchGame = async () => {
    if (!importedPack) return;
    setSwitching(true);
    try {
      await cmd.setActiveGame(importedPack.game_id);
      useAppStore.getState().setActiveGame(importedPack.game_id);
      await runComparison(importedPack);
    } catch (e) {
      toastError(`Couldn't switch game: ${e}`);
    } finally {
      setSwitching(false);
    }
  };

  const handleGetMissingFiles = async () => {
    if (!importedPack) return;
    if (session?.session_type !== "Client") {
      toastError("Connect to a host first (on the Dashboard), then come back and get missing files.");
      return;
    }
    setGettingFiles(true);
    try {
      const plan = await cmd.computePackSyncPlan(importedPack);
      setSyncPlan(plan);
      const unavailable = plan.pack_unavailable?.length ?? 0;
      addLog(
        `Pack sync ready: ${plan.actions.length} file(s) to sync${unavailable ? `, ${unavailable} unavailable from this host` : ""}`,
        "info",
      );
      setPage("dashboard");
    } catch (e) {
      toastError(`Couldn't prepare the sync: ${e}`);
    } finally {
      setGettingFiles(false);
    }
  };

  const closeImport = () => {
    setImportedPack(null);
    setComparison(null);
  };

  return (
    <div className="space-y-5">
      <SectionHeader
        label={<><b>// Share</b> &nbsp;my exact setup</>}
        title={<>Mod<span className="text-neon">packs</span></>}
        description="Export what you have as a small, shareable manifest — no file contents. A friend imports it, sees what they're missing, and pulls just those files from you."
      />

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
        {/* Export */}
        <Panel tone="accent" label={<b>// Export</b>} title="Build a pack">
          <div className="space-y-3">
            <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={128} placeholder="Pack name (e.g. My CC Set)..." aria-label="Pack name" />
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              maxLength={1024}
              placeholder="Description (optional)..."
              aria-label="Pack description"
              rows={2}
              className="input !h-auto py-2 resize-none"
            />
            {contentTypes.length > 0 && (
              <div>
                <p className="hud-label mb-1.5">// Content types (none selected = everything)</p>
                <div className="flex flex-wrap gap-1.5">
                  {contentTypes.map((ct) => (
                    <button
                      key={ct.id}
                      onClick={() => toggleType(ct.id)}
                      className={cx(
                        "h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border transition-colors",
                        selectedTypes.has(ct.id) ? "bg-neon/10 border-neon text-neon" : "bg-bg border-line-hi text-txt-dim hover:text-txt",
                      )}
                    >
                      {ct.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <Button variant="primary" block onClick={handleCreate} disabled={creating || !name.trim()} icon={creating ? <Loader2 size={14} className="animate-spin" /> : <Share2 size={14} />}>
              {creating ? "Building..." : "Build Pack"}
            </Button>

            {exportedPack && (
              <div className="pt-3 border-t border-line space-y-2">
                <p className="text-xs text-txt-dim">
                  <span className="text-txt tabular">{exportedPack.files.length}</span> files, exact bytes stay with you — this only shares the list.
                </p>
                <div className="flex gap-2">
                  <Button size="sm" onClick={handleSaveFile} icon={<SaveIcon size={12} />}>
                    Save .scpack
                  </Button>
                  <Button size="sm" variant="secondary" onClick={handleCopyLink} icon={<Copy size={12} />}>
                    Copy Link
                  </Button>
                </div>
              </div>
            )}
          </div>
        </Panel>

        {/* Import */}
        <Panel tone="accent" label={<b>// Import</b>} title="Get a friend's pack">
          <div className="space-y-3">
            <Button block onClick={handleImportFile} icon={<Upload size={14} />}>
              Choose a .scpack File
            </Button>
            <p className="text-center font-mono text-[10px] uppercase tracking-[0.08em] text-txt-muted">— or drop one anywhere, or paste a link —</p>
            <div className="flex gap-2">
              <Input value={linkInput} onChange={(e) => setLinkInput(e.target.value)} placeholder="synccrate://pack/..." aria-label="Pack link" mono />
              <Button onClick={handleImportLink} disabled={!linkInput.trim()} icon={<Link2 size={12} />}>
                Load
              </Button>
            </div>
          </div>
        </Panel>
      </div>

      {comparing && (
        <Banner tone="info" icon={<Loader2 size={14} className="animate-spin" />} title="Comparing against your files..." />
      )}

      {importedPack && comparison && !comparing && (
        <Panel
          tone={comparison.wrong_game ? "warn" : "accent"}
          brackets
          label={<><b>// Compare</b> &nbsp;Pack vs installed</>}
          title={comparison.pack_name}
          actions={
            <Button size="sm" variant="ghost" onClick={closeImport} icon={<X size={13} />}>
              Close
            </Button>
          }
        >
          {comparison.wrong_game ? (
            <Banner tone="warn" icon={<AlertTriangle size={14} />} title={`This pack is for ${gameLabel(comparison.pack_game)}, but you have ${gameLabel(gameId)} open.`}>
              <Button size="sm" variant="primary" className="mt-2" onClick={handleSwitchGame} disabled={switching}>
                {switching ? "Switching..." : `Switch to ${gameLabel(comparison.pack_game)} and compare`}
              </Button>
            </Banner>
          ) : (
            <>
              <div className="grid grid-cols-3 gap-3">
                <StatTile value={comparison.have} label="Have" highlight />
                <StatTile value={comparison.missing.length} label="Missing" className={comparison.missing.length ? "[&_p]:text-status-red" : undefined} />
                <StatTile value={comparison.different.length} label="Different" className={comparison.different.length ? "[&_p]:text-amber" : undefined} />
              </div>
              <p className="text-xs text-txt-dim mt-2">{formatBytes(comparison.have_bytes)} already on disk.</p>

              {(comparison.missing.length > 0 || comparison.different.length > 0) && (
                <div className="grid grid-cols-2 gap-3 mt-4">
                  {comparison.missing.length > 0 && (
                    <FileGroupBox title="Missing" tone="red" entries={comparison.missing} />
                  )}
                  {comparison.different.length > 0 && (
                    <FileGroupBox title="Different locally" tone="amber" entries={comparison.different} />
                  )}
                </div>
              )}

              {(comparison.missing.length > 0 || comparison.different.length > 0) && (
                <Button
                  variant="primary"
                  className="mt-4"
                  onClick={handleGetMissingFiles}
                  disabled={gettingFiles}
                  icon={gettingFiles ? <Loader2 size={14} className="animate-spin" /> : <ArrowUpDown size={14} />}
                >
                  {gettingFiles ? "Preparing..." : "Get Missing Files From a Host"}
                </Button>
              )}
              {session?.session_type !== "Client" && (
                <p className="text-[11px] text-txt-muted mt-2">
                  Connect to whoever's sharing this pack first (Dashboard → Join), then come back here.
                </p>
              )}
            </>
          )}
        </Panel>
      )}

      {!importedPack && !exportedPack && (
        <EmptyState
          icon={<Share2 size={18} />}
          label="// No pack open"
          title="Build one to share, or import a friend's"
          description="Export your exact setup as a tiny file, or bring in someone else's to see what you're missing."
        />
      )}
    </div>
  );
}

function FileGroupBox({ title, tone, entries }: { title: string; tone: "red" | "amber"; entries: PackFileStatus[] }) {
  const groups = groupByContentType(entries);
  return (
    <div className="bg-bg border border-border">
      <p className={cx("hud-label px-3 py-2 border-b border-border", tone === "red" ? "!text-status-red" : "!text-amber")}>{title}</p>
      <div className="max-h-40 overflow-y-auto px-3 py-2 space-y-1.5">
        {groups.map(([type, files]) => (
          <div key={type}>
            <p className="font-mono text-[10px] uppercase tracking-[0.06em] text-txt-muted">{type} ({files.length})</p>
            {files.slice(0, 20).map((f) => (
              <p key={f.relative_path} className="text-[11px] text-txt-dim font-mono truncate pl-2">
                {f.relative_path.split("/").pop()} <span className="text-txt-muted">· {formatBytes(f.size)}</span>
              </p>
            ))}
            {files.length > 20 && <p className="text-[11px] text-txt-muted pl-2">…and {files.length - 20} more</p>}
          </div>
        ))}
      </div>
    </div>
  );
}
