import { useState, useMemo, useEffect, useCallback } from "react";
import { Search, Package, Tag, CheckSquare, X, Upload, ArrowUpDown, AlertTriangle, Copy, Sparkles, ChevronLeft, ChevronRight } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import ModItem from "./ModItem";
import SaveItem from "./SaveItem";
import ModDetailsPanel from "./ModDetailsPanel";
import ConflictResolver from "./ConflictResolver";
import DuplicateFinder from "./DuplicateFinder";
import { Banner, Button, EmptyState, Input, SectionHeader, cx } from "./ui";
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

  // Per-tab file counts for the HUD tab strip.
  const tabCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    if (!manifest) return counts;
    const byType: Record<string, number> = {};
    for (const f of Object.values(manifest.files)) byType[f.file_type] = (byType[f.file_type] || 0) + 1;
    for (const ct of contentTypes) {
      const types = new Set<string>([ct.file_type, ...Object.values(ct.classify_by_extension ?? {})]);
      let n = 0;
      for (const t of types) n += byType[t] || 0;
      counts[ct.id] = n;
    }
    return counts;
  }, [manifest, contentTypes]);

  return (
    <div className="space-y-4">
      <SectionHeader
        label={<><b>// Content</b> &nbsp;{activeCt?.label ?? "Files"} · {files.length} item{files.length !== 1 ? "s" : ""}</>}
        title={<>{gameDef?.label ?? gameId} <span className="text-neon">Content</span></>}
        actions={
          <>
            {outdatedInTab > 0 && outdated.patchTime !== null && (
              <button
                onClick={() => setOutdatedOnly(!outdatedOnly)}
                title={`Script mods older than the last game update (${formatDateShort(outdated.patchTime)}) often break`}
                className={cx(
                  "btn btn-sm",
                  outdatedOnly ? "bg-amber text-bg" : "text-amber bg-amber/10 hover:bg-amber/20",
                )}
              >
                <AlertTriangle size={12} />
                May be outdated ({outdatedInTab})
              </button>
            )}
            {isModLike && (
              <Button
                size="sm"
                variant={showDuplicates ? "primary" : "secondary"}
                onClick={() => setShowDuplicates(!showDuplicates)}
                icon={<Copy size={12} />}
              >
                Find duplicates
              </Button>
            )}
            {isModLike && (
              <Button
                size="sm"
                variant={bulkMode ? "primary" : "secondary"}
                onClick={() => { setBulkMode(!bulkMode); setSelected(new Set()); }}
                icon={bulkMode ? <X size={12} /> : <CheckSquare size={12} />}
              >
                {bulkMode ? "Cancel" : "Bulk Select"}
              </Button>
            )}
          </>
        }
      />

      {legacyCount > 0 && legacyDismissed !== gameId && (
        <Banner
          tone="warn"
          icon={<AlertTriangle size={14} />}
          title={`${legacyCount} mod${legacyCount !== 1 ? "s" : ""} in _Disabled ${legacyCount !== 1 ? "are" : "is"} still being loaded by ${gameDef?.label ?? gameId}`}
          actions={
            <>
              <Button
                size="sm"
                variant="secondary"
                onClick={fixLegacy}
                disabled={fixingLegacy}
                title="Rename them to .disabled (keeping their folders) so the game really ignores them"
              >
                {fixingLegacy ? "Fixing…" : "Fix"}
              </Button>
              <button onClick={() => setLegacyDismissed(gameId)} className="p-1 text-txt-muted hover:text-txt" aria-label="Dismiss">
                <X size={13} />
              </button>
            </>
          }
        >
          The game loads subfolders, so these mods aren't really disabled.
        </Banner>
      )}

      {showDuplicates && <DuplicateFinder gameId={gameId} onClose={() => setShowDuplicates(false)} />}

      {/* Content type tabs + sort */}
      <div className="flex items-end justify-between gap-4 border-b border-border">
        <div className="flex items-end gap-0 overflow-x-auto -mb-px">
          {contentTypes.length > 1 &&
            contentTypes.map((ct) => {
              const active = activeTab === ct.id;
              return (
                <button
                  key={ct.id}
                  onClick={() => { setActiveContentTab(ct.id); setSearch(""); setTagFilter(null); }}
                  className={cx(
                    "flex items-center gap-2 px-3.5 h-9 border-b-2 font-display font-semibold uppercase tracking-[0.06em] text-[12px] whitespace-nowrap transition-colors",
                    active ? "border-neon text-txt" : "border-transparent text-txt-muted hover:text-txt hover:border-line-hi",
                  )}
                >
                  {ct.label}
                  <span className={cx("font-mono font-normal text-[10px] tracking-normal tabular", active ? "text-neon" : "text-txt-muted")}>
                    {tabCounts[ct.id] ?? 0}
                  </span>
                </button>
              );
            })}
        </div>
        <label className="flex items-center gap-2 pb-2 shrink-0">
          <ArrowUpDown size={12} className="text-txt-muted" />
          <span className="hud-label">Sort</span>
          <select
            value={sortBy}
            onChange={(e) => setSortBy(e.target.value as SortBy)}
            className="bg-bg border border-line-hi h-7 px-2 font-mono text-[11px] uppercase tracking-[0.06em] text-txt focus:outline-none focus:border-neon cursor-pointer"
          >
            <option value="name">Name</option>
            <option value="size">Size</option>
            <option value="date">Date</option>
            <option value="status">Status</option>
          </select>
        </label>
      </div>

      <div className="flex items-center gap-3">
        <Input
          icon={<Search size={15} />}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={`Search ${activeCt?.label ?? "files"}...`}
          wrapperClassName="flex-1"
        />
        {isModLike && !bulkMode && !search && (
          <p className="hud-label flex items-center gap-1.5 shrink-0">
            <Upload size={12} />
            Drop files here to install
          </p>
        )}
      </div>

      {isModLike && allTags.length > 0 && (
        <div className="flex items-center gap-1.5 overflow-x-auto pb-1">
          <Tag size={13} className="text-txt-muted shrink-0 mr-1" />
          <button
            onClick={() => setTagFilter(null)}
            className={cx("tag h-[22px] px-2 transition-colors", tagFilter === null ? "text-neon bg-neon/10" : "tag-neutral hover:text-txt")}
          >
            All Tags
          </button>
          {allTags.map((tag) => (
            <button
              key={tag}
              onClick={() => setTagFilter(tagFilter === tag ? null : tag)}
              className={cx("tag h-[22px] px-2 transition-colors", tagFilter === tag ? "text-neon bg-neon/10" : "tag-neutral hover:text-txt")}
            >
              {tag} <span className="opacity-60">{tagCounts[tag] || 0}</span>
            </button>
          ))}
        </div>
      )}

      {bulkMode && selected.size > 0 && (
        <div className="flex items-center gap-3 border-l-2 border-l-neon bg-neon/[0.06] px-3 py-2">
          <span className="font-mono text-[11px] uppercase tracking-[0.08em] text-neon">{selected.size} selected</span>
          {!bulkTagInput ? (
            <Button size="sm" variant="primary" onClick={() => setBulkTagInput(true)} icon={<Tag size={11} />}>
              Tag Selected
            </Button>
          ) : (
            <div className="flex items-center gap-1.5 flex-wrap">
              {predefinedTags.slice(0, 6).map((tag) => (
                <button key={tag} onClick={() => handleBulkTag(tag)} className="tag tag-neutral h-[22px] px-2 hover:text-neon hover:border-neon transition-colors">
                  {tag}
                </button>
              ))}
              <button onClick={() => setBulkTagInput(false)} className="p-1 text-txt-muted hover:text-txt" aria-label="Cancel"><X size={12} /></button>
            </div>
          )}
        </div>
      )}

      {conflicts.length > 0 && (
        <div className="space-y-3">
          {conflicts.length > 1 && (
            <div className="flex justify-end">
              <Button
                size="sm"
                variant="primary"
                onClick={() => resolveAll("use_newest")}
                title="Resolve every conflict by keeping whichever copy was modified more recently (ties keep yours)"
                icon={<Sparkles size={12} />}
              >
                Keep newer for all
              </Button>
            </div>
          )}
          {conflicts.map((c) => (
            <ConflictResolver key={c.local.relative_path} localFile={c.local} remoteFile={c.remote} onResolve={(resolution) => resolve(c.local.relative_path, resolution)} />
          ))}
        </div>
      )}

      {isScanning && files.length === 0 ? (
        <div className="box divide-y divide-border">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i} className="flex items-center gap-3 px-3 py-2.5">
              <div className="w-7 h-7 animate-pulse bg-bg-card-hover" />
              <div className="flex-1 space-y-1.5">
                <div className="h-2.5 w-1/3 animate-pulse bg-bg-card-hover" />
                <div className="h-2 w-1/5 animate-pulse bg-bg-card-hover" />
              </div>
              <div className="h-[18px] w-16 animate-pulse bg-bg-card-hover" />
            </div>
          ))}
        </div>
      ) : files.length === 0 ? (
        <EmptyState
          icon={<Package size={18} />}
          label={<><b>//</b> 0 results</>}
          title={`No ${activeCt?.label?.toLowerCase() ?? "files"} found`}
          description="Make sure your game folder path is correct."
          action={
            <Button size="sm" onClick={() => useAppStore.getState().navigateToGlobal("settings")}>
              Go to Settings
            </Button>
          }
        />
      ) : (
        // Plain hairline box, not a clipped .panel: the tag editor popover
        // hangs out of its row and clip-path would cut it off.
        <div className="box">
          <div className="flex items-center gap-3 px-3 h-8 border-b border-border bg-bg-2 hud-label !text-[10px]">
            {bulkMode && isModLike && <span className="w-[14px]" />}
            <span className="w-7" />
            <span className="flex-1">Name</span>
            {isModLike ? (
              <>
                <span className="w-[72px] text-right">Size</span>
                <span className="w-[70px]">Hash</span>
                <span className="w-5" />
                <span className="w-7" />
              </>
            ) : (
              <>
                <span className="w-[150px]">Modified</span>
                <span className="w-[72px] text-right">Size</span>
              </>
            )}
            <span className="w-[92px] text-right">Status</span>
          </div>
          <div className="divide-y divide-border">
            {isModLike
              ? pageFiles.map((file) => (
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
              : pageFiles.map((file) => (
                  <SaveItem key={file.relative_path} file={file} syncStatus={getSyncStatus(file.relative_path)} />
                ))}
          </div>
        </div>
      )}

      {files.length > ITEMS_PER_PAGE && (
        <div className="flex items-center justify-between">
          <span className="font-mono text-[11px] text-txt-muted tabular">
            {pageStart + 1}{"–"}{Math.min(pageStart + ITEMS_PER_PAGE, files.length)} <span className="text-txt-muted/70">of</span> {files.length}
          </span>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => setPage((p) => Math.max(0, p - 1))} disabled={safePage === 0} icon={<ChevronLeft size={12} />}>
              Previous
            </Button>
            <span className="font-mono text-[11px] text-txt-dim tabular px-1">
              {String(safePage + 1).padStart(2, "0")} / {String(totalPages).padStart(2, "0")}
            </span>
            <Button size="sm" onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))} disabled={safePage >= totalPages - 1}>
              Next <ChevronRight size={12} />
            </Button>
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
