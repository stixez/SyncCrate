import { ArrowUp, ArrowDown, AlertTriangle, Trash2, Puzzle, Save, Palette } from "lucide-react";
import type { SyncAction } from "../lib/types";
import { formatBytes } from "../lib/utils";
import { cx } from "./ui";

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
  const folder = info.path.slice(0, Math.max(0, info.path.length - fileName.length - 1));

  return (
    <label
      className={cx(
        "group flex items-center gap-2.5 px-2.5 py-1.5 border-l-2 hover:bg-bg-card-hover transition-colors cursor-pointer",
        info.direction === "conflict" ? "border-l-amber" : info.direction === "delete" ? "border-l-status-red" : "border-l-transparent",
        excluded && "opacity-45",
      )}
    >
      <input type="checkbox" checked={!excluded} onChange={() => onToggle(info.path)} className="check" />
      {info.direction === "upload" && <ArrowUp size={12} className="text-neon shrink-0" aria-label="Upload" />}
      {info.direction === "download" && <ArrowDown size={12} className="text-accent-light shrink-0" aria-label="Download" />}
      {info.direction === "conflict" && <AlertTriangle size={12} className="text-amber shrink-0" aria-label="Conflict" />}
      {info.direction === "delete" && <Trash2 size={12} className="text-status-red shrink-0" aria-label="Delete" />}
      {info.fileType === "Mod" ? (
        <Puzzle size={12} className="text-txt-muted shrink-0" />
      ) : info.fileType === "Save" ? (
        <Save size={12} className="text-amber shrink-0" />
      ) : (
        <Palette size={12} className="text-txt-muted shrink-0" />
      )}
      <span className="text-xs truncate flex-1 min-w-0" title={info.path}>
        <span className={cx("text-txt", excluded && "line-through")}>{fileName}</span>
        {folder && <span className="font-mono text-[10px] text-txt-muted ml-2">{folder}</span>}
      </span>
      <span className="font-mono text-[10.5px] text-txt-dim tabular shrink-0">{formatBytes(info.size)}</span>
    </label>
  );
}
