import { useState, useMemo, useEffect, useCallback, useRef, type CSSProperties, type ReactNode } from "react";
import {
  Search, Package, Tag, CheckSquare, X, Upload, ArrowUpDown, AlertTriangle, Copy, Sparkles,
  ChevronRight, Folder, FolderTree, List, Power, PowerOff, ChevronsDownUp, ChevronsUpDown, Info,
} from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import ModItem, { COL, ModIcon } from "./ModItem";
import SaveItem from "./SaveItem";
import ModDetailsPanel from "./ModDetailsPanel";
import CompatIssues from "./CompatIssues";
import ModUpdates, { canCheckUpdates } from "./ModUpdates";
import ConflictResolver from "./ConflictResolver";
import DuplicateFinder from "./DuplicateFinder";
import { Banner, Button, EmptyState, Input, SectionHeader, StatTile, cx } from "./ui";
import { useSync } from "../hooks/useSync";
import { useVirtualList } from "../hooks/useVirtualList";
import { toastSuccess, toastError, toastInfo } from "../lib/toast";
import { dirOf, fileKind, fileName, formatBytes, formatDateShort, isDisabledPath } from "../lib/utils";
import { demoOutdatedScripts, isDemoMode } from "../lib/demoData";
import * as cmd from "../lib/commands";
import type { FileInfo, FileManifest, ModCompatibility, ModMeta, ModUpdate } from "../lib/types";
import { clearModIconCache, metaLookup } from "../lib/modMeta";

type SortBy = "name" | "size" | "date" | "status";
type View = "folders" | "flat";
type SyncStatus = "synced" | "pending" | "conflict" | "local";
type StatusKey = "enabled" | "disabled" | "outdated" | "conflict" | "pending";

// Rows are fixed-height for the virtual list; these match the density pref.
const ROW_H = { comfortable: 40, compact: 30 } as const;
const HEAD_H = { comfortable: 38, compact: 30 } as const;

// Folders/Flat per game + content type ("sims4:mods"): saves want flat, mods
// want folders, and a user's choice for one tab shouldn't flip the others.
const VIEW_KEY = "synccrate-content-view";

function readViews(): Record<string, View> {
  try {
    const raw = JSON.parse(localStorage.getItem(VIEW_KEY) || "{}");
    return raw && typeof raw === "object" ? raw : {};
  } catch {
    return {};
  }
}

function writeViews(v: Record<string, View>) {
  try {
    localStorage.setItem(VIEW_KEY, JSON.stringify(v));
  } catch {
    // Storage unavailable — ignore
  }
}

/** `?demo&view=flat|folders` for screenshots (headless runs start with empty storage). */
function demoView(): View | null {
  if (!isDemoMode()) return null;
  const v = new URLSearchParams(window.location.search).get("view");
  return v === "flat" || v === "folders" ? v : null;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

const STATUS_LABELS: Record<StatusKey, string> = {
  enabled: "Enabled",
  disabled: "Disabled",
  outdated: "Outdated",
  conflict: "Conflicts",
  pending: "To sync",
};

interface Group {
  dir: string;
  files: FileInfo[];
  size: number;
  disabled: number;
  newest: number;
}

type ListItem = { type: "group"; group: Group } | { type: "file"; file: FileInfo; indent: boolean };

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
  const density = useAppStore((s) => s.appearance.density);
  const { resolve, resolveAll } = useSync();

  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<SortBy>("name");
  const [predefinedTags, setPredefinedTags] = useState<string[]>([]);
  const [bulkMode, setBulkMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkTagInput, setBulkTagInput] = useState(false);
  const [detailFile, setDetailFile] = useState<FileInfo | null>(null);
  const [statusFilter, setStatusFilter] = useState<Set<StatusKey>>(new Set());
  const [tagFilter, setTagFilter] = useState<Set<string>>(new Set());
  const [kindFilter, setKindFilter] = useState<Set<string>>(new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [views, setViews] = useState<Record<string, View>>(readViews);
  /** Paths being enabled/disabled right now; one job at a time keeps rescans sane. */
  const [busyPaths, setBusyPaths] = useState<Set<string> | null>(null);
  const [showDuplicates, setShowDuplicates] = useState(false);
  const [legacyCount, setLegacyCount] = useState(0);
  const [legacyDismissed, setLegacyDismissed] = useState<string | null>(null);
  const [fixingLegacy, setFixingLegacy] = useState(false);
  const [outdated, setOutdated] = useState<{ patchTime: number | null; paths: Set<string> }>({ patchTime: null, paths: new Set() });
  /** Game whose activation the backend refused (mid-session): its files are read-only here. */
  const [refusedFor, setRefusedFor] = useState<string | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const activeGame = useAppStore((s) => s.activeGame);
  const searchRef = useRef<HTMLInputElement>(null);

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
      .then(() => { useAppStore.getState().setActiveGame(gameId); setRefusedFor(null); })
      .catch(() => {
        setRefusedFor(gameId);
        cmd.getActiveGame().then((g) => useAppStore.getState().setActiveGame(g)).catch(() => {});
      });
  }, [gameId]);

  // Toggle/delete/install act on the backend's active game; while it's another
  // game (session running), this page must not offer them for this one.
  const readOnly = !isDemoMode() && refusedFor === gameId && activeGame !== gameId;
  const activeLabel = getGameDef(activeGame)?.label ?? activeGame;

  // Re-scan when switching games or when manifest is missing
  useEffect(() => {
    // Ignore results that land after switching to another game (stale manifest).
    let cancelled = false;
    setScanError(null);
    cmd.scanFiles(gameId)
      .then((m) => { if (!cancelled) setManifest(m); })
      .catch((e) => { if (!cancelled) setScanError(String(e)); });
    return () => { cancelled = true; };
  }, [gameId, setManifest]);

  useEffect(() => {
    cmd.checkCompatibility(gameId).then(setModCompatibility).catch(() => {});
  }, [manifest, gameId, setModCompatibility]);

  // Workshop-installed mods (tModLoader) live outside the game folder; say so,
  // or an empty Mods list looks like a detection bug (GitHub issue #2).
  const [workshopMods, setWorkshopMods] = useState(0);
  useEffect(() => {
    let cancelled = false;
    if (!gameDef?.steam_workshop_app_id) { setWorkshopMods(0); return; }
    cmd.getWorkshopModCount(gameId).then((n) => { if (!cancelled) setWorkshopMods(n); }).catch(() => {});
    return () => { cancelled = true; };
  }, [gameId, gameDef?.steam_workshop_app_id]);

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
    let cancelled = false;
    if (isDemoMode()) {
      const r = demoOutdatedScripts(manifest);
      setOutdated({ patchTime: r.patch_time, paths: new Set(r.paths) });
      return;
    }
    if (!hasVersionDetection) { setOutdated({ patchTime: null, paths: new Set() }); return; }
    // Only the active game has a current manifest to compare.
    if (readOnly) { setOutdated({ patchTime: null, paths: new Set() }); return; }
    cmd.getOutdatedScripts(gameId)
      .then((r) => { if (!cancelled) setOutdated({ patchTime: r.patch_time, paths: new Set(r.paths) }); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [manifest, gameId, hasVersionDetection, readOnly]);

  // Mod names/versions/icons from the mods' own metadata files. Only the
  // active game has a current manifest, so read-only pages get none.
  const [modMetas, setModMetas] = useState<ModMeta[]>([]);
  useEffect(() => {
    let cancelled = false;
    clearModIconCache();
    if (readOnly || !manifest) { setModMetas([]); return; }
    cmd.getModMetadata(gameId).then((m) => { if (!cancelled) setModMetas(m); }).catch(() => {});
    return () => { cancelled = true; };
  }, [manifest, gameId, readOnly]);
  const metaFor = useMemo(() => metaLookup(modMetas), [modMetas]);
  const updateReport = useAppStore((s) => s.modUpdates[gameId]);
  const updateFor = useMemo(() => {
    const byKey = new Map((updateReport?.updates ?? []).map((u) => [u.key, u]));
    return (key?: string) => (key ? byKey.get(key) : undefined);
  }, [updateReport]);

  useEffect(() => {
    cmd.getModTags(gameId).then(setModTags).catch(console.error);
    cmd.getPredefinedTags().then(setPredefinedTags).catch(() => {});
  }, [gameId, setModTags]);

  // "/" or Ctrl+F jumps to search, like the game browser.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if ((e.key === "/" && !typing) || ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "f")) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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

  // Determine if this content type looks like "mods" (has tagging, details panel)
  const isModLike = !!activeCt && (activeCt.file_type === "Mod" || activeCt.file_type === "CustomContent" || activeCt.file_type === "Addon");
  // toggle_mod only moves files inside the first content type's folder.
  const isFirstCt = isModLike && activeCt?.id === contentTypes[0]?.id;
  // SMAPI / KSP load every subfolder and mods are folders: no safe per-file toggle.
  const toggleUnsupported = isFirstCt && gameDef?.disable_method === "none";
  const canToggle = isFirstCt && !toggleUnsupported && !readOnly;

  const handleTagsChanged = useCallback((path: string, tags: string[]) => {
    const current = useAppStore.getState().modTags;
    setModTags({ ...current, [path]: tags });
  }, [setModTags]);

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
      await cmd.bulkSetTags(gameId, paths, [tag]);
      const updated = await cmd.getModTags(gameId);
      setModTags(updated);
      setBulkTagInput(false);
      setSelected(new Set());
      setBulkMode(false);
      toastSuccess(`Tagged ${paths.length} file(s) as "${tag}"`);
    } catch (e) {
      toastError(`Bulk tag failed: ${e}`);
    }
  };

  // One pass over the plan instead of scanning every action per row (20k rows).
  const statusMap = useMemo(() => {
    const m = new Map<string, SyncStatus>();
    if (!syncPlan) return m;
    for (const a of syncPlan.actions) {
      if (a.Conflict) {
        m.set(a.Conflict.local.relative_path, "conflict");
        m.set(a.Conflict.remote.relative_path, "conflict");
      }
      const p = a.SendToRemote?.relative_path ?? a.ReceiveFromRemote?.relative_path;
      if (p && !m.has(p)) m.set(p, "pending");
    }
    return m;
  }, [syncPlan]);

  const getSyncStatus = useCallback(
    (path: string): SyncStatus => (syncPlan ? statusMap.get(path) ?? "synced" : "local"),
    [syncPlan, statusMap],
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

  // --- Filtering -------------------------------------------------------------

  const tabFiles = useMemo(() => {
    if (!manifest) return [];
    return Object.values(manifest.files).filter((f) => activeFileTypes.has(f.file_type));
  }, [manifest, activeFileTypes]);

  const hasStatus = useCallback(
    (f: FileInfo, k: StatusKey): boolean => {
      switch (k) {
        case "enabled": return !isDisabledPath(f.relative_path);
        case "disabled": return isDisabledPath(f.relative_path);
        case "outdated": return outdated.paths.has(f.relative_path);
        case "conflict": return getSyncStatus(f.relative_path) === "conflict";
        case "pending": return getSyncStatus(f.relative_path) === "pending";
      }
    },
    [outdated, getSyncStatus],
  );

  // Tab-wide numbers: the summary strip and which chips exist at all.
  const tabStats = useMemo(() => {
    const s = { size: 0, disabled: 0, outdated: 0, conflict: 0, pending: 0 };
    const dirs = new Set<string>();
    const tags = new Set<string>();
    const kinds = new Set<string>();
    for (const f of tabFiles) {
      s.size += f.size;
      if (isDisabledPath(f.relative_path)) s.disabled++;
      if (outdated.paths.has(f.relative_path)) s.outdated++;
      const st = getSyncStatus(f.relative_path);
      if (st === "conflict") s.conflict++;
      else if (st === "pending") s.pending++;
      dirs.add(dirOf(f.relative_path));
      for (const t of modTags[f.relative_path] ?? []) tags.add(t);
      kinds.add(fileKind(f.relative_path));
    }
    return { ...s, dirs: dirs.size, tags: [...tags].sort(), kinds: [...kinds].sort() };
  }, [tabFiles, outdated, getSyncStatus, modTags]);

  const statusKeys = useMemo(() => {
    const keys: StatusKey[] = [];
    if (isModLike && tabStats.disabled > 0) keys.push("enabled", "disabled");
    if (tabStats.outdated > 0) keys.push("outdated");
    if (tabStats.conflict > 0) keys.push("conflict");
    if (tabStats.pending > 0) keys.push("pending");
    return keys;
  }, [isModLike, tabStats]);

  const searched = useMemo(() => {
    const q = search.trim().toLowerCase();
    return q
      ? tabFiles.filter((f) => f.relative_path.toLowerCase().includes(q) || !!metaFor(f.relative_path)?.name.toLowerCase().includes(q))
      : tabFiles;
  }, [tabFiles, search, metaFor]);

  // OR within a chip group, AND across groups.
  const matchStatus = useCallback(
    (f: FileInfo) => statusFilter.size === 0 || [...statusFilter].some((k) => hasStatus(f, k)),
    [statusFilter, hasStatus],
  );
  const matchTags = useCallback(
    (f: FileInfo) => tagFilter.size === 0 || (modTags[f.relative_path] ?? []).some((t) => tagFilter.has(t)),
    [tagFilter, modTags],
  );
  const matchKind = useCallback(
    (f: FileInfo) => kindFilter.size === 0 || kindFilter.has(fileKind(f.relative_path)),
    [kindFilter],
  );

  // Each chip counts what it would show given the *other* groups' filters.
  const facets = useMemo(() => {
    const status: Record<StatusKey, number> = { enabled: 0, disabled: 0, outdated: 0, conflict: 0, pending: 0 };
    let statusAll = 0;
    const tags = new Map<string, number>();
    const kinds = new Map<string, number>();
    for (const f of searched) {
      const s = matchStatus(f), t = matchTags(f), k = matchKind(f);
      if (t && k) {
        statusAll++;
        for (const key of statusKeys) if (hasStatus(f, key)) status[key]++;
      }
      if (s && k) for (const tag of modTags[f.relative_path] ?? []) tags.set(tag, (tags.get(tag) ?? 0) + 1);
      if (s && t) {
        const kind = fileKind(f.relative_path);
        kinds.set(kind, (kinds.get(kind) ?? 0) + 1);
      }
    }
    return { status, statusAll, tags, kinds };
  }, [searched, matchStatus, matchTags, matchKind, statusKeys, hasStatus, modTags]);

  const visible = useMemo(() => {
    return searched
      .filter((f) => matchStatus(f) && matchTags(f) && matchKind(f))
      .sort((a, b) => {
        switch (sortBy) {
          case "size": return b.size - a.size;
          case "date": return b.modified - a.modified;
          case "status": return getSyncStatus(a.relative_path).localeCompare(getSyncStatus(b.relative_path)) || a.relative_path.localeCompare(b.relative_path);
          // By file name (not path) so the flat view isn't just folder order again.
          default: return collator.compare(fileName(a.relative_path), fileName(b.relative_path)) || a.relative_path.localeCompare(b.relative_path);
        }
      });
  }, [searched, matchStatus, matchTags, matchKind, sortBy, getSyncStatus]);

  const filtersActive = search.trim() !== "" || statusFilter.size > 0 || tagFilter.size > 0 || kindFilter.size > 0;
  const clearFilters = () => {
    setSearch("");
    setStatusFilter(new Set());
    setTagFilter(new Set());
    setKindFilter(new Set());
  };

  const switchTab = (id: string) => {
    setActiveContentTab(id);
    clearFilters();
    setCollapsed(new Set());
    setSelected(new Set());
  };

  // --- View + virtual list items ---------------------------------------------

  const viewKey = `${gameId}:${activeTab}`;
  const view: View = views[viewKey] ?? demoView() ?? (tabStats.dirs > 1 ? "folders" : "flat");
  const setView = (v: View) => {
    const next = { ...views, [viewKey]: v };
    setViews(next);
    writeViews(next);
  };

  const groups = useMemo(() => {
    if (view !== "folders") return [];
    const byDir = new Map<string, Group>();
    // `visible` is already sorted, so each group keeps that order.
    for (const f of visible) {
      const dir = dirOf(f.relative_path);
      let g = byDir.get(dir);
      if (!g) { g = { dir, files: [], size: 0, disabled: 0, newest: 0 }; byDir.set(dir, g); }
      g.files.push(f);
      g.size += f.size;
      if (isDisabledPath(f.relative_path)) g.disabled++;
      if (f.modified > g.newest) g.newest = f.modified;
    }
    const list = [...byDir.values()];
    const byName = (a: Group, b: Group) => a.dir.localeCompare(b.dir);
    if (sortBy === "size") return list.sort((a, b) => b.size - a.size || byName(a, b));
    if (sortBy === "date") return list.sort((a, b) => b.newest - a.newest || byName(a, b));
    return list.sort(byName);
  }, [view, visible, sortBy]);

  const rowH = ROW_H[density];
  const headH = HEAD_H[density];
  const { items, heights } = useMemo(() => {
    const items: ListItem[] = [];
    if (view === "flat") {
      for (const f of visible) items.push({ type: "file", file: f, indent: false });
    } else {
      for (const g of groups) {
        items.push({ type: "group", group: g });
        if (!collapsed.has(g.dir)) for (const f of g.files) items.push({ type: "file", file: f, indent: true });
      }
    }
    return { items, heights: items.map((it) => (it.type === "group" ? headH : rowH)) };
  }, [view, visible, groups, collapsed, rowH, headH]);

  const toggleCollapsed = useCallback((dir: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(dir)) next.delete(dir);
      else next.add(dir);
      return next;
    });
  }, []);
  const allCollapsed = groups.length > 0 && groups.every((g) => collapsed.has(g.dir));

  // --- Enable / disable ------------------------------------------------------

  const togglePaths = useCallback(async (paths: string[], enable: boolean) => {
    // Only files not already in the requested state; skips a needless rescan.
    const targets = paths.filter((p) => isDisabledPath(p) === enable);
    if (targets.length === 0 || busyPaths) return;
    const verb = enable ? "Enabled" : "Disabled";
    const label = targets.length === 1 ? fileName(targets[0]) : `${targets.length} files`;

    // No backend in demo mode: rename in the demo manifest so the UI can be exercised.
    if (isDemoMode()) {
      const m = useAppStore.getState().manifest;
      if (!m) return;
      const files: FileManifest["files"] = { ...m.files };
      for (const p of targets) {
        const f = files[p];
        if (!f) continue;
        delete files[p];
        const np = enable ? p.replace(/\.disabled$/i, "") : `${p}.disabled`;
        files[np] = { ...f, relative_path: np };
      }
      setManifest({ ...m, files });
      setSelected(new Set());
      toastSuccess(`${verb} ${label}`);
      return;
    }

    setBusyPaths(new Set(targets));
    let ok = 0;
    const errors: string[] = [];
    try {
      for (const p of targets) {
        try {
          await cmd.toggleMod(gameId, p, enable);
          ok++;
        } catch (e) {
          errors.push(`${fileName(p)}: ${e}`);
        }
      }
      // Paths change on toggle (.disabled rename / _Disabled move): rescan once
      // at the end and drop the now-stale selection.
      setManifest(await cmd.scanFiles(gameId));
      cmd.getModTags(gameId).then(setModTags).catch(() => {});
    } catch (e) {
      errors.push(`Rescan failed: ${e}`);
    } finally {
      setBusyPaths(null);
      setSelected(new Set());
    }
    if (ok) toastSuccess(ok === targets.length ? `${verb} ${label}` : `${verb} ${ok} of ${targets.length} files`);
    if (errors.length) toastError(errors.length === 1 ? errors[0] : `${errors.length} file(s) failed: ${errors[0]}`);
  }, [busyPaths, gameId, setManifest, setModTags]);

  const handleToggle = useCallback((path: string, enable: boolean) => { togglePaths([path], enable); }, [togglePaths]);
  const handleShowDetails = useCallback((f: FileInfo) => setDetailFile(f), []);

  const selectPaths = (paths: string[], on: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      for (const p of paths) {
        if (on) next.add(p);
        else next.delete(p);
      }
      return next;
    });

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

  const toggleIn = <T,>(set: Set<T>, v: T): Set<T> => {
    const next = new Set(set);
    if (next.has(v)) next.delete(v);
    else next.add(v);
    return next;
  };

  const renderItem = (it: ListItem, style: CSSProperties) => {
    if (it.type === "group") {
      const g = it.group;
      const paths = g.files.map((f) => f.relative_path);
      const nSelected = bulkMode ? paths.reduce((n, p) => n + (selected.has(p) ? 1 : 0), 0) : 0;
      return (
        <FolderHeader
          key={`g:${g.dir}`}
          style={style}
          group={g}
          meta={metaFor(g.dir)}
          update={updateFor(metaFor(g.dir)?.key)}
          open={!collapsed.has(g.dir)}
          onToggleOpen={() => toggleCollapsed(g.dir)}
          bulkMode={bulkMode && isModLike}
          selectedCount={nSelected}
          onSelectAll={(on) => selectPaths(paths, on)}
          canToggle={canToggle}
          busy={!!busyPaths}
          working={!!busyPaths && paths.some((p) => busyPaths.has(p))}
          onEnableAll={() => togglePaths(paths, true)}
          onDisableAll={() => togglePaths(paths, false)}
        />
      );
    }
    const f = it.file;
    const p = f.relative_path;
    if (!isModLike) {
      return <SaveItem key={p} style={style} file={f} syncStatus={getSyncStatus(p)} showDir={view === "flat"} indent={it.indent} />;
    }
    return (
      <ModItem
        key={p}
        style={style}
        file={f}
        meta={metaFor(p)}
        update={metaFor(p)?.is_file ? updateFor(metaFor(p)?.key) : undefined}
        syncStatus={getSyncStatus(p)}
        tags={modTags[p]}
        onTagsChanged={handleTagsChanged}
        bulkMode={bulkMode}
        selected={selected.has(p)}
        onSelect={handleSelect}
        compatibility={compatMap.get(p)}
        onShowDetails={handleShowDetails}
        outdatedSince={outdated.paths.has(p) && outdated.patchTime !== null ? outdated.patchTime : undefined}
        showDir={view === "flat"}
        indent={it.indent}
        canToggle={canToggle}
        toggleBusy={!!busyPaths}
        onToggle={handleToggle}
      />
    );
  };

  const visiblePaths = () => visible.map((f) => f.relative_path);
  const allVisibleSelected = visible.length > 0 && visible.every((f) => selected.has(f.relative_path));
  const selectedDisabled = useMemo(() => [...selected].filter(isDisabledPath).length, [selected]);

  return (
    <div className="space-y-4">
      <SectionHeader
        label={<><b>// Content</b> &nbsp;{activeCt?.label ?? "Files"} · {tabFiles.length} item{tabFiles.length !== 1 ? "s" : ""}</>}
        title={<>{gameDef?.label ?? gameId} <span className="text-neon">Content</span></>}
        actions={
          <>
            {/* Opt-in per game: identical files in different folders are normal
                for many games (shared addon libs, BepInEx DLLs, world files). */}
            {isModLike && gameDef?.duplicate_finder && !readOnly && (
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
                onClick={() => { setBulkMode(!bulkMode); setSelected(new Set()); setBulkTagInput(false); }}
                icon={bulkMode ? <X size={12} /> : <CheckSquare size={12} />}
              >
                {bulkMode ? "Cancel" : "Bulk Select"}
              </Button>
            )}
          </>
        }
      />

      {readOnly && (
        <Banner tone="warn" icon={<AlertTriangle size={14} />} title={`You're in a session for ${activeLabel}`}>
          Disconnect to manage {gameDef?.label ?? gameId}'s files. Until then this page is read-only.
        </Banner>
      )}

      {scanError && (
        <Banner tone="warn" icon={<AlertTriangle size={14} />} title="Couldn't scan this game's folder">
          {scanError}
        </Banner>
      )}

      {!readOnly && <CompatIssues gameId={gameId} />}

      {!readOnly && isModLike && canCheckUpdates(modMetas) && <ModUpdates gameId={gameId} metas={modMetas} />}

      {workshopMods > 0 && (
        <Banner tone="info" icon={<Info size={14} />} title={`${workshopMods} of your ${gameDef?.label ?? gameId} mods come from the Steam Workshop`}>
          Steam keeps Workshop mods in its own folder, outside the game folder, so SyncCrate can't list, sync or back them up.
          Friends can subscribe to the same mods on the Workshop. SyncCrate still syncs your worlds, players,{" "}
          <span className="font-mono text-[11px]">enabled.json</span> (which mods are turned on) and any mod files in the Mods folder.
        </Banner>
      )}

      {legacyCount > 0 && legacyDismissed !== gameId && !readOnly && (
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

      {showDuplicates && gameDef?.duplicate_finder && !readOnly && (
        <DuplicateFinder gameId={gameId} onClose={() => setShowDuplicates(false)} />
      )}

      {/* Content type tabs + sort */}
      <div className="flex items-end justify-between gap-4 border-b border-border">
        <div className="flex items-end gap-0 overflow-x-auto -mb-px">
          {contentTypes.length > 1 &&
            contentTypes.map((ct) => {
              const active = activeTab === ct.id;
              return (
                <button
                  key={ct.id}
                  onClick={() => switchTab(ct.id)}
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

      {/* Summary strip: one row of compact tiles, tab-wide (not filtered) */}
      {tabFiles.length > 0 && (
        <div className="grid grid-flow-col auto-cols-fr gap-2">
          <StatTile compact highlight value={tabFiles.length.toLocaleString()} label="Files" hint={`${tabStats.dirs} folder${tabStats.dirs !== 1 ? "s" : ""}`} />
          <StatTile compact value={formatBytes(tabStats.size)} label="Total size" hint={`avg ${formatBytes(tabStats.size / tabFiles.length)}`} />
          {(canToggle || tabStats.disabled > 0) && (
            <StatTile
              compact
              value={tabStats.disabled.toLocaleString()}
              label="Disabled"
              hint={`${Math.round((tabStats.disabled / tabFiles.length) * 100)}% of files`}
            />
          )}
          {tabStats.outdated > 0 && (
            <StatTile
              compact
              value={<span className="text-amber">{tabStats.outdated.toLocaleString()}</span>}
              label="Outdated"
              icon={<AlertTriangle size={12} className="text-amber" />}
              hint={outdated.patchTime !== null
                ? `pre-patch (${new Date(outdated.patchTime * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" })})`
                : "older than last patch"}
            />
          )}
          {syncPlan && (
            <StatTile compact value={tabStats.pending.toLocaleString()} label="To sync" hint="differ from host" />
          )}
          {syncPlan && (
            <StatTile
              compact
              value={<span className={tabStats.conflict > 0 ? "text-status-red" : undefined}>{tabStats.conflict}</span>}
              label="Conflicts"
              hint={tabStats.conflict > 0 ? "need a decision" : "none"}
            />
          )}
        </div>
      )}

      {/* Filter console: search + view, then chip groups (OR within, AND across) */}
      <div className="panel panel-sunken px-4 py-3 space-y-3">
        <div className="flex items-center gap-3 flex-wrap">
          <Input
            ref={searchRef}
            type="text"
            icon={<Search size={15} />}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setSearch("")}
            placeholder={`Search ${activeCt?.label?.toLowerCase() ?? "files"}...`}
            aria-label="Search files"
            wrapperClassName="flex-1 min-w-[220px]"
          />
          <div className="flex border border-border" role="group" aria-label="View">
            <Segment active={view === "folders"} onClick={() => setView("folders")} title="Group by folder">
              <FolderTree size={13} /> Folders
            </Segment>
            <Segment active={view === "flat"} onClick={() => setView("flat")} title="Flat list">
              <List size={13} /> Flat
            </Segment>
          </div>
        </div>

        {(statusKeys.length > 0 || tabStats.kinds.length > 1) && (
          <div className="flex items-center gap-1.5 flex-wrap">
            {statusKeys.length > 0 && (
              <>
                <span className="hud-label mr-1.5"><b>//</b> Status</span>
                <Chip on={statusFilter.size === 0} count={facets.statusAll} onClick={() => setStatusFilter(new Set())}>All</Chip>
                {statusKeys.map((k) => (
                  <Chip
                    key={k}
                    on={statusFilter.has(k)}
                    count={facets.status[k]}
                    onClick={() => setStatusFilter(toggleIn(statusFilter, k))}
                    title={k === "outdated" && outdated.patchTime !== null
                      ? `Script mods older than the last game update (${formatDateShort(outdated.patchTime)}) often break`
                      : undefined}
                  >
                    {k === "outdated" && <AlertTriangle size={10} className={statusFilter.has(k) ? undefined : "text-amber"} />}
                    {k === "conflict" && <span className={cx("w-1.5 h-1.5", statusFilter.has(k) ? "bg-current" : "bg-status-red")} />}
                    {STATUS_LABELS[k]}
                  </Chip>
                ))}
              </>
            )}
            {statusKeys.length > 0 && tabStats.kinds.length > 1 && <span className="w-px h-4 bg-border mx-2" />}
            {tabStats.kinds.length > 1 && (
              <>
                <span className="hud-label mr-1.5"><b>//</b> Kind</span>
                {tabStats.kinds.map((k) => (
                  <Chip key={k || "none"} on={kindFilter.has(k)} count={facets.kinds.get(k) ?? 0} onClick={() => setKindFilter(toggleIn(kindFilter, k))}>
                    {k || "no ext"}
                  </Chip>
                ))}
              </>
            )}
          </div>
        )}

        {isModLike && tabStats.tags.length > 0 && (
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="hud-label mr-1.5 flex items-center gap-1.5"><Tag size={11} /><b>//</b> Tags</span>
            {tabStats.tags.map((t) => (
              <Chip key={t} on={tagFilter.has(t)} count={facets.tags.get(t) ?? 0} onClick={() => setTagFilter(toggleIn(tagFilter, t))}>
                #{t}
              </Chip>
            ))}
          </div>
        )}

        {filtersActive && (
          <div className="flex items-center justify-between gap-3 -mb-1">
            <span className="font-mono text-[11px] text-txt-muted tabular">
              <span className="text-txt">{visible.length.toLocaleString()}</span> of {tabFiles.length.toLocaleString()} match
            </span>
            <button onClick={clearFilters} className="btn btn-ghost btn-sm !text-txt-dim hover:!text-txt">
              <X size={12} />
              Clear filters
            </button>
          </div>
        )}
      </div>

      {bulkMode && isModLike && (
        <div className="flex items-center gap-3 flex-wrap border-l-2 border-l-neon bg-neon/[0.06] px-3 py-2">
          <span className="font-mono text-[11px] uppercase tracking-[0.08em] text-neon tabular">{selected.size} selected</span>
          <Button size="sm" variant="ghost" onClick={() => selectPaths(visiblePaths(), !allVisibleSelected)} disabled={visible.length === 0}>
            {allVisibleSelected ? "Deselect all" : `Select all ${visible.length.toLocaleString()}`}
          </Button>
          <span className="flex-1" />
          {selected.size > 0 && canToggle && (
            <>
              <Button size="sm" variant="secondary" disabled={!!busyPaths || selectedDisabled === 0} onClick={() => togglePaths([...selected], true)} icon={<Power size={11} />}>
                Enable
              </Button>
              <Button size="sm" variant="secondary" disabled={!!busyPaths || selectedDisabled === selected.size} onClick={() => togglePaths([...selected], false)} icon={<PowerOff size={11} />}>
                Disable
              </Button>
            </>
          )}
          {selected.size > 0 && (!bulkTagInput ? (
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
          ))}
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

      {isScanning && tabFiles.length === 0 ? (
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
      ) : tabFiles.length === 0 ? (
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
      ) : visible.length === 0 ? (
        <EmptyState
          icon={<Search size={18} />}
          label="// No match"
          title="No files match these filters"
          description={search.trim() ? <>Nothing matches "{search}" with the current filters.</> : "Try another status, kind or tag."}
          action={<Button size="sm" onClick={clearFilters}>Clear filters</Button>}
        />
      ) : (
        // Plain hairline box, not a clipped .panel: the tag editor popover
        // hangs out of its row and clip-path would cut it off.
        <div className="box">
          {/* -top-6 cancels <main>'s py-6 so the header pins flush to its top edge. */}
          <div className="sticky -top-6 z-30 flex items-center gap-3 pl-3 pr-3 h-8 border-b border-border bg-bg-2 hud-label !text-[10px]">
            {bulkMode && isModLike && <span className="w-[14px] shrink-0" />}
            <span className="w-6 shrink-0" />
            <span className="flex-1 flex items-center gap-3 min-w-0">
              {view === "folders" ? `Folder / name · ${groups.length} folders` : "Name"}
              {view === "folders" && groups.length > 1 && (
                <button
                  onClick={() => setCollapsed(allCollapsed ? new Set() : new Set(groups.map((g) => g.dir)))}
                  className="flex items-center gap-1 text-txt-muted hover:text-txt transition-colors"
                >
                  {allCollapsed ? <ChevronsUpDown size={11} /> : <ChevronsDownUp size={11} />}
                  {allCollapsed ? "Expand all" : "Collapse all"}
                </button>
              )}
            </span>
            {view === "flat" && <span className={COL.dir}>Folder</span>}
            <span className={COL.size}>Size</span>
            <span className={COL.modified}>Modified</span>
            {isModLike && <span className={cx(COL.tags, "text-right")}>Tags</span>}
            <span className={cx(COL.status, "!block text-right")}>Status</span>
            {canToggle && <span className={cx(COL.toggle, "!block text-right")}>On</span>}
          </div>
          <VirtualList items={items} heights={heights} renderItem={renderItem} />
        </div>
      )}

      {tabFiles.length > 0 && (
        <div className="flex items-center justify-between gap-3 font-mono text-[11px] text-txt-muted">
          <span className="tabular">
            {visible.length.toLocaleString()} file{visible.length !== 1 ? "s" : ""}
            {view === "folders" && <> in {groups.length} folder{groups.length !== 1 ? "s" : ""}</>}
          </span>
          {toggleUnsupported && (
            <span
              className="hud-label"
              title={`${gameDef?.label ?? "This game"} loads every subfolder and its mods are folders, so single files can't be switched off safely. Move a mod's folder out of the game to disable it.`}
            >
              Enable/disable not available for this game
            </span>
          )}
          {isModLike && !bulkMode && !readOnly && (
            <span className="hud-label flex items-center gap-1.5">
              <Upload size={12} />
              Drop files anywhere to install
            </span>
          )}
        </div>
      )}

      {detailFile && (
        <ModDetailsPanel
          gameId={gameId}
          canToggle={canToggle}
          file={detailFile}
          meta={metaFor(detailFile.relative_path)}
          update={updateFor(metaFor(detailFile.relative_path)?.key)}
          syncStatus={getSyncStatus(detailFile.relative_path)}
          tags={modTags[detailFile.relative_path] || []}
          compatibility={compatMap.get(detailFile.relative_path)}
          onClose={() => setDetailFile(null)}
        />
      )}
    </div>
  );
}

function VirtualList({
  items,
  heights,
  renderItem,
}: {
  items: ListItem[];
  heights: number[];
  renderItem: (it: ListItem, style: CSSProperties) => ReactNode;
}) {
  const { ref, offsets, total, start, end } = useVirtualList(heights);
  const rows: ReactNode[] = [];
  // `top`, not transform: a transform would make each row a stacking context
  // and trap the tag editor popover under the rows after it.
  for (let i = start; i < end && i < items.length; i++) rows.push(renderItem(items[i], { top: offsets[i], height: heights[i] }));
  // -mb-px: the last row's bottom hairline sits on the box border instead of doubling it.
  return (
    <div ref={ref} className="relative -mb-px" style={{ height: total }}>
      {rows}
    </div>
  );
}

function FolderHeader({
  style,
  group,
  meta,
  update,
  open,
  onToggleOpen,
  bulkMode,
  selectedCount,
  onSelectAll,
  canToggle,
  busy,
  working,
  onEnableAll,
  onDisableAll,
}: {
  style: CSSProperties;
  group: Group;
  meta?: ModMeta;
  update?: ModUpdate;
  open: boolean;
  onToggleOpen: () => void;
  bulkMode: boolean;
  selectedCount: number;
  onSelectAll: (on: boolean) => void;
  canToggle: boolean;
  busy: boolean;
  working: boolean;
  onEnableAll: () => void;
  onDisableAll: () => void;
}) {
  const n = group.files.length;
  const slash = group.dir.lastIndexOf("/");
  const parent = slash >= 0 ? group.dir.slice(0, slash + 1) : "";
  const leaf = slash >= 0 ? group.dir.slice(slash + 1) : group.dir || "(top level)";
  const allSelected = selectedCount === n;
  const enabled = n - group.disabled;
  // A folder header names the mod only for folder mods (a jar's name is on its own row).
  const folderMeta = meta && !meta.is_file ? meta : undefined;

  return (
    <div
      style={style}
      role="button"
      tabIndex={0}
      aria-expanded={open}
      onClick={onToggleOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggleOpen(); }
      }}
      className={cx(
        "group absolute inset-x-0 flex items-center gap-3 pl-3 pr-3 border-b border-border bg-bg-2 cursor-pointer select-none transition-colors hover:bg-bg-card-hover",
        "before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px]",
        selectedCount > 0 ? "before:bg-neon" : "before:bg-line-hi",
      )}
    >
      {bulkMode && (
        <input
          type="checkbox"
          className="check shrink-0"
          checked={allSelected}
          ref={(el) => { if (el) el.indeterminate = selectedCount > 0 && !allSelected; }}
          onChange={() => onSelectAll(!allSelected)}
          onClick={(e) => e.stopPropagation()}
          aria-label={`Select all in ${group.dir || "top level"}`}
        />
      )}
      <span className="w-6 shrink-0 flex items-center justify-center text-txt-muted">
        <ChevronRight size={13} className={cx("transition-transform", open && "rotate-90")} />
      </span>
      {folderMeta ? (
        <>
          <ModIcon meta={folderMeta} size={18} className="-ml-2" fallback={<Folder size={13} className={open ? "text-accent-light" : "text-txt-muted"} />} />
          <p className="min-w-0 truncate text-[12.5px]" title={`${folderMeta.name} · ${group.dir}`}>
            <span className="text-txt font-semibold">{folderMeta.name}</span>
            {folderMeta.version && <span className="font-mono text-[10.5px] text-txt-dim ml-1.5">v{folderMeta.version.replace(/^v/i, "")}</span>}
            {update && <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-neon ml-2" title={`Update available: ${update.latest}`}>update {update.latest}</span>}
            <span className="font-mono text-[11px] text-txt-muted ml-2">{group.dir}</span>
          </p>
        </>
      ) : (
        <>
          <Folder size={13} className={cx("shrink-0 -ml-2", open ? "text-accent-light" : "text-txt-muted")} />
          <p className="min-w-0 truncate font-mono text-[12px]" title={group.dir}>
            <span className="text-txt-muted">{parent}</span>
            <span className="text-txt font-semibold">{leaf}</span>
          </p>
        </>
      )}
      <span className="shrink-0 font-mono text-[10.5px] text-txt-muted tabular whitespace-nowrap">
        {n.toLocaleString()} file{n !== 1 ? "s" : ""}
        <span className="text-line-hi mx-1.5">/</span>
        {formatBytes(group.size)}
        {group.disabled > 0 && (
          <>
            <span className="text-line-hi mx-1.5">/</span>
            <span className="text-txt-dim">{group.disabled} off</span>
          </>
        )}
      </span>
      <span className="flex-1" />
      {working && <span className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-neon">Working…</span>}
      {canToggle && !working && (
        <span className="flex items-center gap-1 shrink-0" onClick={(e) => e.stopPropagation()}>
          {group.disabled > 0 && (
            <button onClick={onEnableAll} disabled={busy} className="btn btn-ghost btn-sm !h-6 !px-2 !text-[10.5px] !text-txt-dim hover:!text-neon disabled:opacity-40" title={`Enable ${group.disabled} disabled file${group.disabled !== 1 ? "s" : ""}`}>
              <Power size={11} /> Enable all
            </button>
          )}
          {enabled > 0 && (
            <button onClick={onDisableAll} disabled={busy} className="btn btn-ghost btn-sm !h-6 !px-2 !text-[10.5px] !text-txt-dim hover:!text-txt disabled:opacity-40" title={`Disable ${enabled} file${enabled !== 1 ? "s" : ""}`}>
              <PowerOff size={11} /> Disable all
            </button>
          )}
        </span>
      )}
    </div>
  );
}

function Chip({ on, count, onClick, title, children }: { on: boolean; count: number; onClick: () => void; title?: string; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={on}
      title={title}
      disabled={count === 0 && !on}
      className={cx(
        "tag !py-1 !px-2 transition-colors disabled:opacity-35 disabled:cursor-default",
        on ? "!text-neon-ink bg-neon !border-neon" : "text-txt-dim hover:text-txt hover:!border-line-hi",
      )}
    >
      {children}
      <span className={cx("tabular", on ? "opacity-70" : "text-txt-muted")}>{count.toLocaleString()}</span>
    </button>
  );
}

function Segment({ active, onClick, title, children }: { active: boolean; onClick: () => void; title?: string; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      title={title}
      className={cx(
        "flex items-center gap-1.5 h-[38px] px-3 font-display font-semibold uppercase tracking-[0.06em] text-[11.5px] transition-colors border-r border-border last:border-r-0",
        active ? "bg-neon text-neon-ink" : "text-txt-dim hover:text-txt hover:bg-bg-card-hover",
      )}
    >
      {children}
    </button>
  );
}
