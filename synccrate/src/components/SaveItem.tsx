import { memo, type CSSProperties } from "react";
import { Save } from "lucide-react";
import type { FileInfo } from "../lib/types";
import { dirOf, fileName, formatBytes, formatDate, formatRelative } from "../lib/utils";
import StatusBadge from "./StatusBadge";
import { COL } from "./ModItem";
import { cx } from "./ui";

interface SaveItemProps {
  file: FileInfo;
  style?: CSSProperties;
  syncStatus?: "synced" | "pending" | "conflict" | "local";
  showDir?: boolean;
  indent?: boolean;
}

function SaveItem({ file, style, syncStatus = "local", showDir, indent }: SaveItemProps) {
  const name = fileName(file.relative_path);

  return (
    <div
      style={style}
      className={cx(
        "group absolute inset-x-0 flex items-center gap-3 pr-3 border-b border-border transition-colors hover:bg-bg-card-hover before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px] before:bg-transparent hover:before:bg-neon",
        indent ? "pl-9" : "pl-3",
      )}
    >
      <div className="w-6 h-6 shrink-0 grid place-items-center border border-line-hi bg-bg text-txt-dim">
        <Save size={12} />
      </div>
      <p className="flex-1 min-w-0 text-[13px] font-medium truncate text-txt" title={file.relative_path}>{name}</p>
      {showDir && (
        <span className={cx(COL.dir, "font-mono text-[11px] text-txt-muted truncate")} title={dirOf(file.relative_path)}>
          {dirOf(file.relative_path) || "/"}
        </span>
      )}
      <span className={cx(COL.size, "font-mono text-[11px] text-txt-dim tabular")}>{formatBytes(file.size)}</span>
      <span className={cx(COL.modified, "font-mono text-[11px] text-txt-muted tabular")} title={file.modified ? formatDate(file.modified) : undefined}>
        {formatRelative(file.modified)}
      </span>
      <div className={COL.status}>
        <StatusBadge status={syncStatus} />
      </div>
    </div>
  );
}

export default memo(SaveItem);
