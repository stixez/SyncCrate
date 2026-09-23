import { Save } from "lucide-react";
import type { FileInfo } from "../lib/types";
import { formatBytes, formatDate } from "../lib/utils";
import StatusBadge from "./StatusBadge";

interface SaveItemProps {
  file: FileInfo;
  syncStatus?: "synced" | "pending" | "conflict" | "local";
}

export default function SaveItem({ file, syncStatus = "local" }: SaveItemProps) {
  const name = file.relative_path.split(/[/\\]/).pop() || file.relative_path;

  return (
    <div className="row-y group relative flex items-center gap-3 px-3 py-2 transition-colors hover:bg-bg-card-hover before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px] before:bg-transparent hover:before:bg-neon">
      <div className="w-7 h-7 shrink-0 grid place-items-center border border-line-hi bg-bg text-txt-dim">
        <Save size={14} />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-[13px] font-medium truncate text-txt">{name}</p>
        <p className="font-mono text-[11px] text-txt-muted truncate">{file.relative_path}</p>
      </div>
      <span className="w-[150px] shrink-0 font-mono text-[11px] text-txt-dim">{formatDate(file.modified)}</span>
      <span className="w-[72px] shrink-0 text-right font-mono text-[11px] text-txt-dim tabular">{formatBytes(file.size)}</span>
      <div className="w-[92px] shrink-0 flex justify-end">
        <StatusBadge status={syncStatus} />
      </div>
    </div>
  );
}
