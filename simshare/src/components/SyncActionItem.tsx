import { ArrowUp, ArrowDown, AlertTriangle, Trash2, Puzzle, Save, Palette } from "lucide-react";
import type { SyncAction } from "../lib/types";
import { formatBytes } from "../lib/utils";

interface SyncActionItemProps {
  action: SyncAction;
  excluded: boolean;
  onToggle: (path: string) => void;
}

function getActionInfo(action: SyncAction) {
  if (action.SendToRemote) {
    return {
      path: action.SendToRemote.relative_path,
      size: action.SendToRemote.size,
      direction: "upload" as const,
      fileType: action.SendToRemote.file_type,
    };
  }
  if (action.ReceiveFromRemote) {
    return {
      path: action.ReceiveFromRemote.relative_path,
      size: action.ReceiveFromRemote.size,
      direction: "download" as const,
      fileType: action.ReceiveFromRemote.file_type,
    };
  }
  if (action.Conflict) {
    return {
      path: action.Conflict.local.relative_path,
      size: action.Conflict.local.size,
      direction: "conflict" as const,
      fileType: action.Conflict.local.file_type,
    };
  }
  if (action.Delete) {
    return {
      path: action.Delete,
      size: 0,
      direction: "delete" as const,
      fileType: "Mod" as const,
    };
  }
  return null;
}

export default function SyncActionItem({ action, excluded, onToggle }: SyncActionItemProps) {
  const info = getActionInfo(action);
  if (!info) return null;

  const fileName = info.path.split(/[/\\]/).pop() || info.path;

  return (
    <label className={`flex items-center gap-2 px-2 py-1.5 rounded hover:bg-bg-card-hover transition-colors cursor-pointer ${excluded ? "opacity-50" : ""}`}>
      <input
        type="checkbox"
        checked={!excluded}
        onChange={() => onToggle(info.path)}
        className="shrink-0 accent-accent"
      />
      {info.fileType === "Mod" ? (
        <Puzzle size={12} className="text-accent-light shrink-0" />
      ) : info.fileType === "Save" ? (
        <Save size={12} className="text-status-yellow shrink-0" />
      ) : (
        <Palette size={12} className="text-pink-400 shrink-0" />
      )}
      {info.direction === "upload" && <ArrowUp size={12} className="text-status-green shrink-0" />}
      {info.direction === "download" && <ArrowDown size={12} className="text-accent-light shrink-0" />}
      {info.direction === "conflict" && <AlertTriangle size={12} className="text-status-yellow shrink-0" />}
      {info.direction === "delete" && <Trash2 size={12} className="text-status-red shrink-0" />}
      <span className="text-xs truncate flex-1" title={info.path}>{fileName}</span>
      <span className="text-[10px] text-txt-dim shrink-0">{formatBytes(info.size)}</span>
    </label>
  );
}
