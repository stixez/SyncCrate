import { X, FolderOpen, Puzzle, Palette, Power, PowerOff, AlertTriangle } from "lucide-react";
import type { FileInfo, ModCompatibility } from "../lib/types";
import { formatBytes, formatDate } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";

interface ModDetailsPanelProps {
  file: FileInfo;
  syncStatus: "synced" | "pending" | "conflict" | "local";
  tags: string[];
  compatibility?: ModCompatibility;
  onClose: () => void;
}

export default function ModDetailsPanel({
  file,
  syncStatus,
  tags,
  compatibility,
  onClose,
}: ModDetailsPanelProps) {
  const gamePaths = useAppStore((s) => s.gamePaths);
  const activeGame = useAppStore((s) => s.activeGame);
  const setManifest = useAppStore((s) => s.setManifest);
  const isMod = file.file_type === "Mod";
  const name = file.relative_path.split(/[/\\]/).pop() || file.relative_path;
  const isDisabled = file.relative_path.includes("_Disabled/") || file.relative_path.includes("_Disabled\\");
  const basePath = gamePaths[activeGame];

  const handleToggle = async () => {
    try {
      await cmd.toggleMod(file.relative_path, isDisabled);
      const m = await cmd.scanFiles();
      setManifest(m);
      toastSuccess(isDisabled ? `Enabled ${name}` : `Disabled ${name}`);
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleReveal = () => {
    if (!basePath) return;
    const parts = file.relative_path.split(/[/\\]/);
    parts.pop(); // remove filename
    const dir = basePath + "/" + parts.join("/");
    cmd.openFolder(dir);
  };

  const statusLabels: Record<string, string> = {
    synced: "Synced",
    pending: "Pending sync",
    conflict: "Conflict",
    local: "Local only",
  };

  const statusColors: Record<string, string> = {
    synced: "text-status-green",
    pending: "text-status-yellow",
    conflict: "text-status-red",
    local: "text-accent-light",
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/80" onClick={onClose}>
      <div
        className="bg-bg-card border border-border rounded-xl p-6 w-full max-w-md shadow-xl space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className={`w-10 h-10 rounded-lg flex items-center justify-center ${isMod ? "bg-accent/20" : "bg-pink-500/20"}`}>
              {isMod ? <Puzzle size={20} className="text-accent-light" /> : <Palette size={20} className="text-pink-400" />}
            </div>
            <div>
              <h3 className="font-semibold text-sm">{name}</h3>
              <p className="text-xs text-txt-dim">{isMod ? "Script Mod" : "Custom Content"}</p>
            </div>
          </div>
          <button onClick={onClose} className="p-1 text-txt-dim hover:text-txt transition-colors">
            <X size={18} />
          </button>
        </div>

        <div className="space-y-2 text-sm">
          <div className="flex justify-between">
            <span className="text-txt-dim">Path</span>
            <span className="text-right max-w-[260px] truncate" title={file.relative_path}>{file.relative_path}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-txt-dim">Size</span>
            <span>{formatBytes(file.size)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-txt-dim">Modified</span>
            <span>{formatDate(file.modified)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-txt-dim">Hash</span>
            <span className="font-mono text-xs">{file.hash || "N/A"}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-txt-dim">Status</span>
            <span className={statusColors[syncStatus]}>{statusLabels[syncStatus]}</span>
          </div>
          {tags.length > 0 && (
            <div className="flex justify-between items-start">
              <span className="text-txt-dim">Tags</span>
              <div className="flex flex-wrap gap-1 justify-end max-w-[200px]">
                {tags.map((tag) => (
                  <span key={tag} className="px-1.5 py-0 rounded-full bg-accent/15 text-accent-light text-[10px] font-medium">
                    {tag}
                  </span>
                ))}
              </div>
            </div>
          )}
          {compatibility?.status === "MissingPacks" && (
            <div className="flex items-start gap-2 bg-status-yellow/10 border border-status-yellow/30 rounded-lg p-2">
              <AlertTriangle size={14} className="text-status-yellow shrink-0 mt-0.5" />
              <div>
                <p className="text-xs font-medium text-status-yellow">Missing Packs</p>
                <p className="text-xs text-txt-dim mt-0.5">
                  {compatibility.missing_packs.map((p) => p.code).join(", ")}
                </p>
              </div>
            </div>
          )}
        </div>

        <div className="flex gap-2 pt-2">
          <button
            onClick={handleToggle}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
              isDisabled
                ? "bg-status-green/20 text-status-green hover:bg-status-green/30"
                : "bg-status-yellow/20 text-status-yellow hover:bg-status-yellow/30"
            }`}
          >
            {isDisabled ? <Power size={14} /> : <PowerOff size={14} />}
            {isDisabled ? "Enable" : "Disable"}
          </button>
          {basePath && (
            <button
              onClick={handleReveal}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg border border-border text-txt-dim hover:bg-bg-card-hover text-sm transition-colors"
            >
              <FolderOpen size={14} />
              Reveal in Explorer
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
