import { useState } from "react";
import { Puzzle, Palette, Tag, AlertTriangle } from "lucide-react";
import type { FileInfo, ModCompatibility } from "../lib/types";
import { formatBytes, formatDateShort, isDisabledPath } from "../lib/utils";
import StatusBadge from "./StatusBadge";
import TagEditor from "./TagEditor";
import { Badge, cx } from "./ui";

interface ModItemProps {
  file: FileInfo;
  syncStatus?: "synced" | "pending" | "conflict" | "local";
  tags?: string[];
  onTagsChanged?: (path: string, tags: string[]) => void;
  selected?: boolean;
  onSelect?: (path: string) => void;
  bulkMode?: boolean;
  compatibility?: ModCompatibility;
  onShowDetails?: () => void;
  /** Game patch time (unix secs) when this script mod predates it. */
  outdatedSince?: number;
}

export default function ModItem({
  file,
  syncStatus = "local",
  tags = [],
  onTagsChanged,
  selected,
  onSelect,
  bulkMode,
  compatibility,
  onShowDetails,
  outdatedSince,
}: ModItemProps) {
  const isMod = file.file_type === "Mod";
  const name = file.relative_path.split(/[/\\]/).pop() || file.relative_path;
  const [showTagEditor, setShowTagEditor] = useState(false);
  const isDisabled = isDisabledPath(file.relative_path);
  const isOutdated = outdatedSince !== undefined;
  const dir = file.relative_path.slice(0, Math.max(0, file.relative_path.length - name.length)).replace(/[/\\]$/, "");

  return (
    <div
      className={cx(
        "row-y group relative flex items-center gap-3 pl-3 pr-3 py-2 cursor-pointer transition-colors hover:bg-bg-card-hover",
        // State rule on the left edge: amber = may be outdated, neon on hover.
        "before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px]",
        isOutdated ? "before:bg-amber" : "before:bg-transparent hover:before:bg-neon",
        selected && "bg-neon/[0.06]",
      )}
      onClick={() => {
        if (!bulkMode && onShowDetails) onShowDetails();
      }}
    >
      {bulkMode && (
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onSelect?.(file.relative_path)}
          onClick={(e) => e.stopPropagation()}
          aria-label={`Select ${name}`}
          className="check"
        />
      )}
      <div
        className={cx(
          "w-7 h-7 shrink-0 grid place-items-center border",
          isDisabled ? "border-border text-txt-muted" : isMod ? "border-accent/50 text-accent-light bg-accent/10" : "border-line-hi text-txt-dim bg-bg",
        )}
        title={isMod ? "Script mod" : "Custom content"}
      >
        {isMod ? <Puzzle size={14} /> : <Palette size={14} />}
      </div>
      <div className={cx("flex-1 min-w-0", isDisabled && "opacity-60")}>
        <div className="flex items-center gap-2">
          <p className={cx("text-[13px] font-medium truncate", isDisabled ? "text-txt-dim line-through decoration-txt-muted" : "text-txt")}>{name}</p>
          {isDisabled && <Badge tone="neutral" className="shrink-0">Disabled</Badge>}
          {isOutdated && (
            <Badge tone="amber" className="shrink-0" title={`Script mods older than the last game update (${formatDateShort(outdatedSince)}) often break`}>
              May be outdated
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2 mt-0.5 min-w-0">
          <p className="font-mono text-[11px] text-txt-muted truncate">{dir || "/"}</p>
          {tags.length > 0 && (
            <div className="flex gap-1 shrink-0">
              {tags.slice(0, 3).map((tag) => (
                <span key={tag} className="font-mono text-[10px] uppercase tracking-[0.06em] text-accent-light">
                  #{tag}
                </span>
              ))}
              {tags.length > 3 && <span className="font-mono text-[10px] text-txt-muted">+{tags.length - 3}</span>}
            </div>
          )}
        </div>
      </div>
      <span className="w-[72px] shrink-0 text-right font-mono text-[11px] text-txt-dim tabular">{formatBytes(file.size)}</span>
      <span className="w-[70px] shrink-0 font-mono text-[11px] text-txt-muted">{file.hash.slice(0, 8)}</span>
      <div className="w-5 shrink-0 flex justify-center">
        {compatibility?.status === "MissingPacks" && (
          <span title={`Missing packs: ${compatibility.missing_packs.map((p) => p.code).join(", ")}`} className="text-amber">
            <AlertTriangle size={14} />
          </span>
        )}
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          setShowTagEditor(!showTagEditor);
        }}
        className={cx(
          "w-7 h-7 shrink-0 grid place-items-center border transition-colors",
          showTagEditor ? "border-neon text-neon" : "border-transparent text-txt-muted hover:border-line-hi hover:text-txt",
        )}
        title="Edit tags"
      >
        <Tag size={13} />
      </button>
      <div className="w-[92px] shrink-0 flex justify-end">
        <StatusBadge status={syncStatus} />
      </div>
      {showTagEditor && onTagsChanged && (
        <TagEditor
          filePath={file.relative_path}
          currentTags={tags}
          onTagsChanged={onTagsChanged}
          onClose={() => setShowTagEditor(false)}
        />
      )}
    </div>
  );
}
