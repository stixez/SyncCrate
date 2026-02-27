import { useState, useMemo, useEffect, useCallback } from "react";
import { Search, Package, Tag, CheckSquare, X, Upload, ArrowUpDown } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import ModItem from "./ModItem";
import ConflictResolver from "./ConflictResolver";
import { useSync } from "../hooks/useSync";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";
import type { ModCompatibility } from "../lib/types";

type ModSortBy = "name" | "size" | "date" | "status";

export default function ModList() {
  const manifest = useAppStore((s) => s.manifest);
  const setManifest = useAppStore((s) => s.setManifest);
  const syncPlan = useAppStore((s) => s.syncPlan);
  const modTags = useAppStore((s) => s.modTags);
  const setModTags = useAppStore((s) => s.setModTags);
  const isScanning = useAppStore((s) => s.isScanning);
  const { resolve } = useSync();
  const search = useAppStore((s) => s.modSearch);
  const setSearch = useAppStore((s) => s.setModSearch);
  const filter = useAppStore((s) => s.modFilter);
  const setFilter = useAppStore((s) => s.setModFilter);
  const tagFilter = useAppStore((s) => s.modTagFilter);
  const setTagFilter = useAppStore((s) => s.setModTagFilter);
  const [sortBy, setSortBy] = useState<ModSortBy>("name");
  const [predefinedTags, setPredefinedTags] = useState<string[]>([]);
  const [bulkMode, setBulkMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkTagInput, setBulkTagInput] = useState(false);
  const modCompatibility = useAppStore((s) => s.modCompatibility);
  const setModCompatibility = useAppStore((s) => s.setModCompatibility);

  useEffect(() => {
    if (!manifest) {
      cmd.scanFiles().then(setManifest).catch(console.error);
    }
  }, [manifest, setManifest]);

  useEffect(() => {
    cmd.checkCompatibility().then(setModCompatibility).catch(() => {});
  }, [manifest, setModCompatibility]);

  useEffect(() => {
    cmd.getModTags().then(setModTags).catch(console.error);
    cmd.getPredefinedTags().then(setPredefinedTags).catch(() => {});
  }, [setModTags]);

  const handleTagsChanged = useCallback(
    (path: string, tags: string[]) => {
      setModTags({ ...modTags, [path]: tags });
    },
    [modTags, setModTags],
  );

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
      toastSuccess(`Tagged ${paths.length} mod(s) as "${tag}"`);
    } catch (e) {
      toastError(`Bulk tag failed: ${e}`);
    }
  };

  const getSyncStatus = useCallback(
    (path: string): "synced" | "pending" | "conflict" | "local" => {
      if (!syncPlan) return "local";
      for (const action of syncPlan.actions) {
        if (action.Conflict && (action.Conflict.local.relative_path === path || action.Conflict.remote.relative_path === path)) {
          return "conflict";
        }
        if (action.SendToRemote && action.SendToRemote.relative_path === path) return "pending";
        if (action.ReceiveFromRemote && action.ReceiveFromRemote.relative_path === path) return "pending";
      }
      return "synced";
    },
    [syncPlan],
  );

  const conflicts = useMemo(() => {
    if (!syncPlan) return [];
    return syncPlan.actions
      .filter((a) => a.Conflict)
      .map((a) => a.Conflict!)
      .filter(
        (c) =>
          c.local.file_type === "Mod" || c.local.file_type === "CustomContent",
      );
  }, [syncPlan]);

  const compatMap = useMemo(() => {
    const map = new Map<string, ModCompatibility>();
    for (const c of modCompatibility) {
      map.set(c.mod_path, c);
    }
    return map;
  }, [modCompatibility]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    Object.values(modTags).forEach((arr) => arr.forEach((t) => tags.add(t)));
    return Array.from(tags).sort();
  }, [modTags]);

  const mods = useMemo(() => {
    if (!manifest) return [];
    return Object.values(manifest.files)
      .filter((f) => f.file_type === "Mod" || f.file_type === "CustomContent")
      .filter((f) => {
        if (filter === "mod") return f.file_type === "Mod";
        if (filter === "cc") return f.file_type === "CustomContent";
        return true;
      })
      .filter((f) =>
        f.relative_path.toLowerCase().includes(search.toLowerCase()),
      )
      .filter((f) => {
        if (!tagFilter) return true;
        const fileTags = modTags[f.relative_path] || [];
        return fileTags.includes(tagFilter);
      })
      .sort((a, b) => {
        switch (sortBy) {
          case "size": return b.size - a.size;
          case "date": return b.modified - a.modified;
          case "status": {
            const sa = getSyncStatus(a.relative_path);
            const sb = getSyncStatus(b.relative_path);
            return sa.localeCompare(sb);
          }
          default: return a.relative_path.localeCompare(b.relative_path);
        }
      });
  }, [manifest, search, filter, tagFilter, modTags, sortBy, getSyncStatus]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">Mods & Custom Content</h2>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 bg-bg-card border border-border rounded-lg px-2.5 py-1">
            <ArrowUpDown size={12} className="text-txt-dim" />
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as ModSortBy)}
              className="bg-transparent text-xs text-txt-dim focus:outline-none cursor-pointer"
            >
              <option value="name">Name</option>
              <option value="size">Size</option>
              <option value="date">Date</option>
              <option value="status">Status</option>
            </select>
          </div>
          <button
            onClick={() => {
              setBulkMode(!bulkMode);
              setSelected(new Set());
            }}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs transition-colors ${
              bulkMode
                ? "bg-accent text-white"
                : "bg-bg-card border border-border text-txt-dim hover:bg-bg-card-hover"
            }`}
          >
            <CheckSquare size={12} />
            {bulkMode ? "Cancel" : "Bulk Select"}
          </button>
          <span className="text-txt-dim text-sm">{mods.length} items</span>
        </div>
      </div>

      <div className="flex gap-3">
        <div className="flex-1 relative">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-txt-dim" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search mods..."
            className="w-full bg-bg-card border border-border rounded-lg pl-9 pr-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
        </div>
        <div className="flex rounded-lg border border-border overflow-hidden">
          {(["all", "mod", "cc"] as const).map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`px-3 py-2 text-xs font-medium transition-colors ${
                filter === f ? "bg-accent text-white" : "bg-bg-card text-txt-dim hover:bg-bg-card-hover"
              }`}
            >
              {f === "all" ? "All" : f === "mod" ? "Scripts" : "CC"}
            </button>
          ))}
        </div>
      </div>

      {allTags.length > 0 && (
        <div className="flex items-center gap-2 overflow-x-auto pb-1">
          <Tag size={14} className="text-txt-dim shrink-0" />
          <button
            onClick={() => setTagFilter(null)}
            className={`px-2 py-0.5 rounded-full text-xs font-medium whitespace-nowrap transition-colors ${
              tagFilter === null
                ? "bg-accent text-white"
                : "bg-bg-card border border-border text-txt-dim hover:border-accent/50"
            }`}
          >
            All Tags
          </button>
          {allTags.map((tag) => (
            <button
              key={tag}
              onClick={() => setTagFilter(tagFilter === tag ? null : tag)}
              className={`px-2 py-0.5 rounded-full text-xs font-medium whitespace-nowrap transition-colors ${
                tagFilter === tag
                  ? "bg-accent text-white"
                  : "bg-bg-card border border-border text-txt-dim hover:border-accent/50"
              }`}
            >
              {tag}
            </button>
          ))}
        </div>
      )}

      {!bulkMode && !search && (
        <p className="flex items-center gap-1.5 text-xs text-txt-dim">
          <Upload size={12} />
          Drop .package files here to install
        </p>
      )}

      {bulkMode && selected.size > 0 && (
        <div className="flex items-center gap-2 bg-accent/10 border border-accent/30 rounded-lg p-2">
          <span className="text-xs font-medium text-accent-light">
            {selected.size} selected
          </span>
          {!bulkTagInput ? (
            <button
              onClick={() => setBulkTagInput(true)}
              className="flex items-center gap-1 px-2 py-0.5 rounded bg-accent text-white text-xs font-medium"
            >
              <Tag size={10} />
              Tag Selected
            </button>
          ) : (
            <div className="flex items-center gap-1.5">
              {predefinedTags.slice(0, 6).map((tag) => (
                <button
                  key={tag}
                  onClick={() => handleBulkTag(tag)}
                  className="px-2 py-0.5 rounded-full bg-bg-card border border-border text-xs text-txt-dim hover:border-accent/50"
                >
                  {tag}
                </button>
              ))}
              <button
                onClick={() => setBulkTagInput(false)}
                className="text-txt-dim hover:text-txt"
              >
                <X size={12} />
              </button>
            </div>
          )}
        </div>
      )}

      {conflicts.length > 0 && (
        <div className="space-y-3">
          {conflicts.map((c) => (
            <ConflictResolver
              key={c.local.relative_path}
              localFile={c.local}
              remoteFile={c.remote}
              onResolve={(resolution) => resolve(c.local.relative_path, resolution)}
            />
          ))}
        </div>
      )}

      <div className="space-y-1">
        {isScanning && mods.length === 0 ? (
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
        ) : mods.length === 0 ? (
          <div className="text-center py-12 text-txt-dim">
            <Package size={40} className="mx-auto mb-3 opacity-40" />
            <p>No mods found</p>
            <p className="text-xs mt-1">Make sure your Sims 4 Mods folder path is correct</p>
          </div>
        ) : (
          mods.map((mod) => (
            <ModItem
              key={mod.relative_path}
              file={mod}
              syncStatus={getSyncStatus(mod.relative_path)}
              tags={modTags[mod.relative_path] || []}
              onTagsChanged={handleTagsChanged}
              bulkMode={bulkMode}
              selected={selected.has(mod.relative_path)}
              onSelect={handleSelect}
              compatibility={compatMap.get(mod.relative_path)}
            />
          ))
        )}
      </div>
    </div>
  );
}
