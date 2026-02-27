import { useState, useMemo, useEffect, useCallback } from "react";
import { Save, Search, ArrowUpDown } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import SaveItem from "./SaveItem";
import ConflictResolver from "./ConflictResolver";
import { useSync } from "../hooks/useSync";
import * as cmd from "../lib/commands";

type SaveSortBy = "date" | "name" | "size";

export default function SaveList() {
  const manifest = useAppStore((s) => s.manifest);
  const setManifest = useAppStore((s) => s.setManifest);
  const syncPlan = useAppStore((s) => s.syncPlan);
  const isScanning = useAppStore((s) => s.isScanning);
  const { resolve } = useSync();
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<SaveSortBy>("date");

  useEffect(() => {
    if (!manifest) {
      cmd.scanFiles().then(setManifest).catch(console.error);
    }
  }, [manifest, setManifest]);

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
      .filter((c) => c.local.file_type === "Save");
  }, [syncPlan]);

  const saves = useMemo(() => {
    if (!manifest) return [];
    return Object.values(manifest.files)
      .filter((f) => f.file_type === "Save")
      .filter((f) => f.relative_path.toLowerCase().includes(search.toLowerCase()))
      .sort((a, b) => {
        switch (sortBy) {
          case "name": return a.relative_path.localeCompare(b.relative_path);
          case "size": return b.size - a.size;
          default: return b.modified - a.modified;
        }
      });
  }, [manifest, search, sortBy]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">Save Files</h2>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 bg-bg-card border border-border rounded-lg px-2.5 py-1">
            <ArrowUpDown size={12} className="text-txt-dim" />
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as SaveSortBy)}
              className="bg-transparent text-xs text-txt-dim focus:outline-none cursor-pointer"
            >
              <option value="date">Date</option>
              <option value="name">Name</option>
              <option value="size">Size</option>
            </select>
          </div>
          <span className="text-txt-dim text-sm">{saves.length} saves</span>
        </div>
      </div>

      <div className="relative">
        <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-txt-dim" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search saves..."
          className="w-full bg-bg-card border border-border rounded-lg pl-9 pr-3 py-2 text-sm focus:outline-none focus:border-accent"
        />
      </div>

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
        {isScanning && saves.length === 0 ? (
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
        ) : saves.length === 0 ? (
          <div className="text-center py-12 text-txt-dim">
            <Save size={40} className="mx-auto mb-3 opacity-40" />
            <p>No save files found</p>
          </div>
        ) : (
          saves.map((save) => (
            <SaveItem
              key={save.relative_path}
              file={save}
              syncStatus={getSyncStatus(save.relative_path)}
            />
          ))
        )}
      </div>
    </div>
  );
}
