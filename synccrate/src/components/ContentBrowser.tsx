import { useState, useMemo, useEffect, useCallback } from "react";
import { Search, Package, Tag, CheckSquare, X, Upload, ArrowUpDown, AlertTriangle, Copy, Sparkles } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import ModItem from "./ModItem";
import SaveItem from "./SaveItem";
import ModDetailsPanel from "./ModDetailsPanel";
import ConflictResolver from "./ConflictResolver";
import DuplicateFinder from "./DuplicateFinder";
import { useSync } from "../hooks/useSync";
import { toastSuccess, toastError, toastInfo } from "../lib/toast";
import { formatDateShort } from "../lib/utils";
import * as cmd from "../lib/commands";
import type { FileInfo, ModCompatibility, ContentTypeDefinition } from "../lib/types";

type SortBy = "name" | "size" | "date" | "status";
const ITEMS_PER_PAGE = 50;

interface Props {
  gameId: string;
}

export default function ContentBrowser({ gameId }: Props) {
  const manifest = useAppStore((s) => s.manifest);
  const setManifest = useAppStore((s) => s.setManifest);
  const syncPlan = useAppStore((s) => s.syncPlan);
  const modTags = useAppStore((s) => s.modTags);
  const setModTags = useAppStore((s) => s.setModTags);
  const isScanning = useAppStore((s) => s.isScanning);
  const activeContentTab = useAppStore((s) => s.activeContentTab);
  const setActiveContentTab = useAppStore((s) => s.setActiveContentTab);
  const { resolve, resolveAll } = useSync();

  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<SortBy>("name");
  const [predefinedTags, setPredefinedTags] = useState<string[]>([]);
  const [bulkMode, setBulkMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkTagInput, setBulkTagInput] = useState(false);
  const [page, setPage] = useState(0);
  const [detailFile, setDetailFile] = useState<FileInfo | null>(null);
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const [showDuplicates, setShowDuplicates] = useState(false);
  const [legacyCount, setLegacyCount] = useState(0);
  const [legacyDismissed, setLegacyDismissed] = useState<string | null>(null);
  const [fixingLegacy, setFixingLegacy] = useState(false);
  const [outdated, setOutdated] = useState<{ patchTime: number | null; paths: Set<string> }>({ patchTime: null, paths: new Set() });
  const [outdatedOnly, setOutdatedOnly] = useState(false);

  const modCompatibility = useAppStore((s) => s.modCompatibility);
  const setModCompatibility = useAppStore((s) => s.setModCompatibility);

  const gameDef = getGameDef(gameId);
  const contentTypes = gameDef?.content_types ?? [];

  // Pick active tab — default to first content type
  const activeTab = activeContentTab && contentTypes.some((ct) => ct.id === activeContentTab)
    ? activeContentTab
    : contentTypes[0]?.id ?? null;

  const activeCt = contentTypes.find((ct) => ct.id === activeTab);

  // Keep backend in sync with the viewed game
  useEffect(() => {
    // Refused mid-session; only mirror into the store once the backend accepts it.
    cmd.setActiveGame(gameId)
      .then(() => useAppStore.getState().setActiveGame(gameId))
      .catch(() => {
        cmd.getActiveGame().then((g) => useAppStore.getState().setActiveGame(g)).catch(() => {});
      });
  }, [gameId]);

  // Re-scan when switching games or when manifest is missing
  useEffect(() => {
    // Ignore results that land after switching to another game (stale manifest).
    let cancelled = false;
    cmd.scanFiles(gameId).then((m) => { if (!cancelled) setManifest(m); }).catch(console.error);
    return () => { cancelled = true; };
  }, [gameId, setManifest]);

  useEffect(() => {
    cmd.checkCompatibility(gameId).then(setModCompatibility).catch(() => {});
  }, [manifest, gameId, setModCompatibility]);

  // Legacy `_Disabled/` folder: The Sims loads subfolders, so those mods still load.
  const renameDisable = gameDef?.disable_method === "rename";
  useEffect(() => {
    if (!renameDisable) { setLegacyCount(0); return; }
    let cancelled = false;
    cmd.countLegacyDisabled(gameId).then((n) => { if (!cancelled) setLegacyCount(n); }).catch(() => {});
    return () => { cancelled = true; };
  }, [manifest, gameId, renameDisable]);

  const fixLegacy = async () => {
    setFixingLegacy(true);
    try {
      const r = await cmd.migrateLegacyDisabled(gameId);
      setManifest(await cmd.scanFiles(gameId));
      setLegacyCount(await cmd.countLegacyDisabled(gameId));
      if (r.moved) toastSuccess(`Disabled ${r.moved} mod${r.moved !== 1 ? "s" : ""} properly (renamed to .disabled)`);
      if (r.collisions.length) toastInfo(`${r.collisions.length} file(s) left in _Disabled: a disabled copy already exists (${r.collisions[0]})`);
      if (r.errors.length) toastError(`${r.errors.length} file(s) could not be moved: ${r.errors[0]}`);
    } catch (e) {
      toastError(`Fix failed: ${e}`);
    } finally {
      setFixingLegacy(false);
    }
  };

  // Script mods older than the last game patch (games with version detection only).
  const hasVersionDetection = !!gameDef?.version_detection;
  useEffect(() => {
    if (!hasVersionDetection) { setOutdated({ patchTime: null, paths: new Set() }); return; }
    let cancelled = false;
    cmd.getOutdatedScripts()
      .then((r) => { if (!cancelled) setOutdated({ patchTime: r.patch_time, paths: new Set(r.paths) }); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [manifest, gameId, hasVersionDetection]);

  useEffect(() => {
    cmd.getModTags().then(setModTags).catch(console.error);
    cmd.getPredefinedTags().then(setPredefinedTags).catch(() => {});
  }, [setModTags]);

  // Collect all file_type values for the active content type
  const activeFileTypes = useMemo(() => {
    if (!activeCt) return new Set<string>();
    const types = new Set<string>([activeCt.file_type]);
    if (activeCt.classify_by_extension) {
      for (const ft of Object.values(activeCt.classify_by_extension)) {
        types.add(ft);
      }
    }
    return types;
  }, [activeCt]);

  const handleTagsChanged = useCallback((path: string, tags: string[]) => {
    setModTags({ ...modTags, [path]: tags });
  }, [modTags, setModTags]);

  const handleSelect = useCallback((path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const handleBulkTag = async (tag: string) => {
    const paths = Array.from(selected);
    if (paths.length === 0) return;
    try {
      await cmd.bulkSetTags(paths, [tag]);
      const updated = await cmd.getModTags();
      setModTags(updated);
      setBulkTagInput(false);
      setSelected(new Set());
      setBulkMode(false);
      toastSuccess(`Tagged ${paths.length} file(s) as "${tag}"`);
    } catch (e) {
      toastError(`Bulk tag failed: ${e}`);
    }
  };

  const getSyncStatus = useCallback(
    (path: string): "synced" | "pending" | "conflict" | "local" => {
      if (!syncPlan) return "local";
      for (const action of syncPlan.actions) {
        if (action.Conflict && (action.Conflict.local.relative_path === path || action.Conflict.remote.relative_path === path)) return "conflict";
        if (action.SendToRemote && action.SendToRemote.relative_path === path) return "pending";
        if (action.ReceiveFromRemote && action.ReceiveFromRemote.relative_path === path) return "pending";
      }
      return "synced";
    },
    [syncPlan],
  );

  const conflicts = useMemo(() => {
    if (!syncPlan || !activeCt) return [];
    return syncPlan.actions
      .filter((a) => a.Conflict)
      .map((a) => a.Conflict!)
      .filter((c) => activeFileTypes.has(c.local.file_type));
  }, [syncPlan, activeCt, activeFileTypes]);

  const compatMap = useMemo(() => {
    const map = new Map<string, ModCompatibility>();
    for (const c of modCompatibility) map.set(c.mod_path, c);
    return map;
  }, [modCompatibility]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    Object.values(modTags).forEach((arr) => arr.forEach((t) => tags.add(t)));
    return Array.from(tags).sort();
  }, [modTags]);

  const tagCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const tags of Object.values(modTags)) {
      for (const t of tags) counts[t] = (counts[t] || 0) + 1;
    }
    return counts;
  }, [modTags]);

  const files = useMemo(() => {
    if (!manifest) return [];
    return Object.values(manifest.files)
      .filter((f) => activeFileTypes.has(f.file_type))
      .filter((f) => f.relative_path.toLowerCase().includes(search.toLowerCase()))
      .filter((f) => {
        if (!tagFilter) return true;
        const fileTags = modTags[f.relative_path] || [];
        return fileTags.includes(tagFilter);
      })
      .filter((f) => !outdatedOnly || outdated.paths.has(f.relative_path))
      .sort((a, b) => {
        switch (sortBy) {
          case "size": return b.size - a.size;
          case "date": return b.modified - a.modified;
          case "status": return getSyncStatus(a.relative_path).localeCompare(getSyncStatus(b.relative_path));
          default: return a.relative_path.localeCompare(b.relative_path);
        }
      });
  }, [manifest, activeFileTypes, search, tagFilter, modTags, sortBy, getSyncStatus, outdatedOnly, outdated]);

  useEffect(() => { setPage(0); }, [search, tagFilter, sortBy, activeTab, outdatedOnly]);

  const outdatedInTab = useMemo(() => {
    if (!manifest || outdated.paths.size === 0) return 0;
    let n = 0;
    for (const p of outdated.paths) {
      const f = manifest.files[p];
      if (f && activeFileTypes.has(f.file_type)) n++;
    }
    return n;
  }, [manifest, outdated, activeFileTypes]);

  const totalPages = Math.max(1, Math.ceil(files.length / ITEMS_PER_PAGE));
  const safePage = Math.min(page, totalPages - 1);
  const pageStart = safePage * ITEMS_PER_PAGE;
  const pageFiles = files.slice(pageStart, pageStart + ITEMS_PER_PAGE);

  // Determine if this content type looks like "mods" (has tagging, details panel)
  const isModLike = activeCt && (activeCt.file_type === "Mod" || activeCt.file_type === "CustomContent" || activeCt.file_type === "Addon");

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">{gameDef?.label ?? gameId} Content</h2>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 bg-bg-card border border-border rounded-lg px-2.5 py-1">
            <ArrowUpDown size={12} className="text-txt-dim" />
            <select value={sortBy} onChange={(e) => setSortBy(e.target.value as SortBy)} className="bg-transparent text-xs text-txt-dim focus:outline-none cursor-pointer">
              <option value="name">Name</option>
              <option value="size">Size</option>
              <option value="date">Date</option>
              <option value="status">Status</option>
            </select>
          </div>
          {outdatedInTab > 0 && outdated.patchTime !== null && (
            <button
              onClick={() => setOutdatedOnly(!outdatedOnly)}
              title={`Script mods older than the last game update (${formatDateShort(outdated.patchTime)}) often break`}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs transition-colors ${outdatedOnly ? "bg-status-yellow text-black" : "bg-status-yellow/15 border border-status-yellow/30 text-status-yellow hover:bg-status-yellow/25"}`}
            >
              <AlertTriangle size={12} />
              May be outdated ({outdatedInTab})
            </button>
          )}
          {isModLike && (
            <button
              onClick={() => setShowDuplicates(!showDuplicates)}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs transition-colors ${showDuplicates ? "bg-accent text-white" : "bg-bg-card border border-border text-txt-dim hover:bg-bg-card-hover"}`}
            >
              <Copy size={12} />
              Find duplicates
            </button>
          )}
          {isModLike && (
            <button
              onClick={() => { setBulkMode(!bulkMode); setSelected(new Set()); }}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs transition-colors ${bulkMode ? "bg-accent text-white" : "bg-bg-card border border-border text-txt-dim hover:bg-bg-card-hover"}`}
            >
              <CheckSquare size={12} />
              {bulkMode ? "Cancel" : "Bulk Select"}
            </button>
          )}
          <span className="text-txt-dim text-sm">{files.length} items</span>
        </div>
      </div>

      {legacyCount > 0 && legacyDismissed !== gameId && (
        <div className="flex items-center gap-2 bg-status-yellow/10 border border-status-yellow/30 rounded-lg px-3 py-2 text-xs">
          <AlertTriangle size={14} className="text-status-yellow shrink-0" />
          <span className="flex-1">
            {legacyCount} mod{legacyCount !== 1 ? "s" : ""} in _Disabled {legacyCount !== 1 ? "are" : "is"} still being loaded by {gameDef?.label ?? gameId}
          </span>
          <button
            onClick={fixLegacy}
            disabled={fixingLegacy}
            title="Rename them to .disabled (keeping their folders) so the game really ignores them"
            className="px-2.5 py-0.5 rounded bg-status-yellow text-black font-medium disabled:opacity-50"
          >
            {fixingLegacy ? "Fixing…" : "Fix"}
          </button>
          <button onClick={() => setLegacyDismissed(gameId)} className="text-txt-dim hover:text-txt" aria-label="Dismiss">
            <X size={12} />
          </button>
        </div>
      )}

      {showDuplicates && <DuplicateFinder gameId={gameId} onClose={() => setShowDuplicates(false)} />}

      {/* Content type tabs */}
      {contentTypes.length > 1 && (
        <div className="flex rounded-lg border border-border overflow-hidden w-fit">
          {contentTypes.map((ct) => (
            <button
              key={ct.id}
              onClick={() => { setActiveContentTab(ct.id); setSearch(""); setTagFilter(null); }}
              className={`px-3 py-1.5 text-xs font-medium transition-colors ${activeTab === ct.id ? "bg-accent text-white" : "bg-bg-card text-txt-dim hover:bg-bg-card-hover"}`}
            >
              {ct.label}
            </button>
          ))}
        </div>
      )}

      <div className="flex gap-3">
        <div className="flex-1 relative">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-txt-dim" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={`Search ${activeCt?.label ?? "files"}...`}
            className="w-full bg-bg-card border border-border rounded-lg pl-9 pr-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
        </div>
      </div>

      {isModLike && allTags.length > 0 && (
        <div className="flex items-center gap-2 overflow-x-auto pb-1">
          <Tag size={14} className="text-txt-dim shrink-0" />
          <button
            onClick={() => setTagFilter(null)}
            className={`px-2 py-0.5 rounded-full text-xs font-medium whitespace-nowrap transition-colors ${tagFilter === null ? "bg-accent text-white" : "bg-bg-card border border-border text-txt-dim hover:border-accent/50"}`}
          >
            All Tags
          </button>
          {allTags.map((tag) => (
            <button
              key={tag}
              onClick={() => setTagFilter(tagFilter === tag ? null : tag)}
              className={`px-2 py-0.5 rounded-full text-xs font-medium whitespace-nowrap transition-colors ${tagFilter === tag ? "bg-accent text-white" : "bg-bg-card border border-border text-txt-dim hover:border-accent/50"}`}
            >
              {tag} ({tagCounts[tag] || 0})
            </button>
          ))}
        </div>
      )}

      {isModLike && !bulkMode && !search && (
        <p className="flex items-center gap-1.5 text-xs text-txt-dim">
          <Upload size={12} />
          Drop files here to install
        </p>
      )}

      {bulkMode && selected.size > 0 && (
        <div className="flex items-center gap-2 bg-accent/10 border border-accent/30 rounded-lg p-2">
          <span className="text-xs font-medium text-accent-light">{selected.size} selected</span>
          {!bulkTagInput ? (
            <button onClick={() => setBulkTagInput(true)} className="flex items-center gap-1 px-2 py-0.5 rounded bg-accent text-white text-xs font-medium">
              <Tag size={10} />
              Tag Selected
            </button>
          ) : (
            <div className="flex items-center gap-1.5">
              {predefinedTags.slice(0, 6).map((tag) => (
                <button key={tag} onClick={() => handleBulkTag(tag)} className="px-2 py-0.5 rounded-full bg-bg-card border border-border text-xs text-txt-dim hover:border-accent/50">{tag}</button>
              ))}
              <button onClick={() => setBulkTagInput(false)} className="text-txt-dim hover:text-txt"><X size={12} /></button>
            </div>
          )}
        </div>
      )}

      {conflicts.length > 0 && (
        <div className="space-y-3">
          {conflicts.length > 1 && (
            <div className="flex justify-end">
              <button
                onClick={() => resolveAll("use_newest")}
                title="Resolve every conflict by keeping whichever copy was modified more recently (ties keep yours)"
                className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-accent hover:bg-accent-light text-white text-xs font-medium transition-colors"
              >
                <Sparkles size={12} />
                Keep newer for all
              </button>
            </div>
          )}
          {conflicts.map((c) => (
            <ConflictResolver key={c.local.relative_path} localFile={c.local} remoteFile={c.remote} onResolve={(resolution) => resolve(c.local.relative_path, resolution)} />
          ))}
        </div>
      )}

      <div className="space-y-1">
        {isScanning && files.length === 0 ? (
          Array.from({ length: 5 }).map((_, i) => (
            <div key={i} className="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-card">
              <div className="w-8 h-8 rounded animate-pulse bg-bg-card-hover" />
              <div className="flex-1 space-y-2">
                <div className="h-3 w-1/3 rounded animate-pulse bg-bg-card-hover" />
                <div className="h-2 w-1/5 rounded animate-pulse bg-bg-card-hover" />
              </div>
              <div className="h-5 w-16 rounded animate-pulse bg-bg-card-hover" />
            </div>
          ))
        ) : files.length === 0 ? (
          <div className="text-center py-12 text-txt-dim">
            <Package size={40} className="mx-auto mb-3 opacity-40" />
            <p>No {activeCt?.label?.toLowerCase() ?? "files"} found</p>
            <p className="text-xs mt-1">Make sure your game folder path is correct</p>
            <button onClick={() => useAppStore.getState().navigateToGlobal("settings")} className="text-accent-light hover:underline text-xs cursor-pointer mt-2 inline-block">
              Go to Settings
            </button>
          </div>
        ) : isModLike ? (
          pageFiles.map((file) => (
            <ModItem
              key={file.relative_path}
              file={file}
              syncStatus={getSyncStatus(file.relative_path)}
              tags={modTags[file.relative_path] || []}
              onTagsChanged={handleTagsChanged}
              bulkMode={bulkMode}
              selected={selected.has(file.relative_path)}
              onSelect={handleSelect}
              compatibility={compatMap.get(file.relative_path)}
              onShowDetails={() => setDetailFile(file)}
              outdatedSince={outdated.paths.has(file.relative_path) && outdated.patchTime !== null ? outdated.patchTime : undefined}
            />
          ))
        ) : (
          pageFiles.map((file) => (
            <SaveItem key={file.relative_path} file={file} syncStatus={getSyncStatus(file.relative_path)} />
          ))
        )}
      </div>

      {files.length > ITEMS_PER_PAGE && (
        <div className="flex items-center justify-between pt-2">
          <span className="text-xs text-txt-dim">
            {pageStart + 1}\u2013{Math.min(pageStart + ITEMS_PER_PAGE, files.length)} of {files.length}
          </span>
          <div className="flex items-center gap-2">
            <button onClick={() => setPage((p) => Math.max(0, p - 1))} disabled={safePage === 0} className="px-3 py-1 rounded-lg bg-bg-card border border-border text-xs font-medium transition-colors hover:bg-bg-card-hover disabled:opacity-40 disabled:cursor-not-allowed">
              Previous
            </button>
            <span className="text-xs text-txt-dim">Page {safePage + 1} / {totalPages}</span>
            <button onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))} disabled={safePage >= totalPages - 1} className="px-3 py-1 rounded-lg bg-bg-card border border-border text-xs font-medium transition-colors hover:bg-bg-card-hover disabled:opacity-40 disabled:cursor-not-allowed">
              Next
            </button>
          </div>
        </div>
      )}

      {detailFile && (
        <ModDetailsPanel
          file={detailFile}
          syncStatus={getSyncStatus(detailFile.relative_path)}
          tags={modTags[detailFile.relative_path] || []}
          compatibility={compatMap.get(detailFile.relative_path)}
          onClose={() => setDetailFile(null)}
        />
      )}
    </div>
  );
}
