import { memo, useState, type CSSProperties, type ReactNode } from "react";
import { Puzzle, Palette, Tag, AlertTriangle } from "lucide-react";
import type { FileInfo, ModCompatibility, ModMeta, ModUpdate } from "../lib/types";
import { useModIcon } from "../lib/modMeta";
import { dirOf, fileName, formatBytes, formatDate, formatDateShort, formatRelative, isDisabledPath } from "../lib/utils";
import StatusBadge from "./StatusBadge";
import TagEditor from "./TagEditor";
import { Badge, Toggle, cx } from "./ui";

/* Column widths shared by the rows and ContentBrowser's column header, so they
 * line up. Literal strings on purpose: Tailwind only keeps classes it finds. */
export const COL = {
  dir: "w-[180px] shrink-0 hidden lg:block",
  size: "w-[64px] shrink-0 text-right",
  modified: "w-[64px] shrink-0 text-right",
  tags: "w-[116px] shrink-0",
  status: "w-[92px] shrink-0 flex justify-end",
  toggle: "w-[40px] shrink-0 flex justify-end",
};

/** A mod's own icon, or `fallback` while loading / when it has none. */
export function ModIcon({ meta, size, className, fallback }: { meta?: ModMeta; size: number; className?: string; fallback: ReactNode }) {
  const url = useModIcon(meta);
  return (
    <span className={cx("shrink-0 grid place-items-center overflow-hidden", className)} style={{ width: size, height: size }}>
      {url ? <img src={url} alt="" className="w-full h-full object-cover" draggable={false} /> : fallback}
    </span>
  );
}

interface ModItemProps {
  file: FileInfo;
  /** Metadata of the mod this file belongs to (its own for jars, its folder's otherwise). */
  meta?: ModMeta;
  /** An available update for this file's own mod (jars). */
  update?: ModUpdate;
  /** Fixed row height from the virtual list (density-dependent). */
  style?: CSSProperties;
  syncStatus?: "synced" | "pending" | "conflict" | "local";
  tags?: string[];
  onTagsChanged?: (path: string, tags: string[]) => void;
  selected?: boolean;
  onSelect?: (path: string) => void;
  bulkMode?: boolean;
  compatibility?: ModCompatibility;
  onShowDetails?: (file: FileInfo) => void;
  /** Game patch time (unix secs) when this script mod predates it. */
  outdatedSince?: number;
  /** Flat view shows the folder column; grouped rows sit under a folder header instead. */
  showDir?: boolean;
  indent?: boolean;
  /** Toggle only works for the first content type's folder (backend toggle_mod). */
  canToggle?: boolean;
  toggleBusy?: boolean;
  onToggle?: (path: string, enable: boolean) => void;
}

function ModItem({
  file,
  meta,
  update,
  style,
  syncStatus = "local",
  tags = [],
  onTagsChanged,
  selected,
  onSelect,
  bulkMode,
  compatibility,
  onShowDetails,
  outdatedSince,
  showDir,
  indent,
  canToggle,
  toggleBusy,
  onToggle,
}: ModItemProps) {
  const isMod = file.file_type === "Mod";
  const name = fileName(file.relative_path);
  const [showTagEditor, setShowTagEditor] = useState(false);
  const isDisabled = isDisabledPath(file.relative_path);
  const isOutdated = outdatedSince !== undefined;
  const missingPacks = compatibility?.status === "MissingPacks";
  // A single-file mod (jar) is named by its metadata; files inside a folder
  // mod keep their file name (the folder header names the mod).
  const ownMeta = meta?.is_file ? meta : undefined;

  return (
    <div
      style={style}
      className={cx(
        "group absolute inset-x-0 flex items-center gap-3 pr-3 border-b border-border cursor-pointer transition-colors hover:bg-bg-card-hover",
        indent ? "pl-9" : "pl-3",
        // State rule on the left edge: amber = may be outdated, neon on hover / selected.
        "before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px]",
        isOutdated ? "before:bg-amber" : selected ? "before:bg-neon" : "before:bg-transparent hover:before:bg-neon",
        selected && "bg-neon/[0.06]",
        // Lift the row with an open tag editor above the rows positioned after it.
        showTagEditor && "z-20",
      )}
      onClick={() => {
        if (bulkMode) onSelect?.(file.relative_path);
        else onShowDetails?.(file);
      }}
    >
      {bulkMode && (
        <input
          type="checkbox"
          checked={!!selected}
          onChange={() => onSelect?.(file.relative_path)}
          onClick={(e) => e.stopPropagation()}
          aria-label={`Select ${name}`}
          className="check shrink-0"
        />
      )}
      <div
        className={cx(
          "w-6 h-6 shrink-0 grid place-items-center border",
          isDisabled ? "border-border text-txt-muted" : isMod ? "border-accent/50 text-accent-light bg-accent/10" : "border-line-hi text-txt-dim bg-bg",
        )}
        title={isMod ? "Script mod" : "Custom content"}
      >
        <ModIcon meta={ownMeta} size={22} fallback={isMod ? <Puzzle size={12} /> : <Palette size={12} />} />
      </div>
      <div className="flex-1 min-w-0 flex items-center gap-2">
        <p
          className={cx("text-[13px] font-medium truncate", isDisabled ? "text-txt-muted line-through decoration-txt-muted/60" : "text-txt")}
          title={ownMeta ? `${ownMeta.name} · ${file.relative_path}` : file.relative_path}
        >
          {ownMeta ? ownMeta.name : name}
          {ownMeta?.version && <span className="font-mono text-[10.5px] text-txt-dim font-normal ml-1.5 no-underline">v{ownMeta.version.replace(/^v/i, "")}</span>}
          {ownMeta && <span className="font-mono text-[10.5px] text-txt-muted font-normal ml-2">{name}</span>}
          {!ownMeta && meta && showDir && <span className="text-[11px] text-txt-muted font-normal ml-2">· {meta.name}</span>}
        </p>
        {isDisabled && !canToggle && <Badge tone="neutral" className="shrink-0">Disabled</Badge>}
        {update && <Badge tone="neon" className="shrink-0" title={`Update available: ${update.latest}`}>Update</Badge>}
        {isOutdated && (
          <Badge tone="amber" className="shrink-0" title={`Script mods older than the last game update (${formatDateShort(outdatedSince)}) often break`}>
            Outdated
          </Badge>
        )}
        {missingPacks && (
          <span title={`Missing packs: ${compatibility!.missing_packs.map((p) => p.code).join(", ")}`} className="text-amber shrink-0">
            <AlertTriangle size={13} />
          </span>
        )}
      </div>
      {showDir && (
        <span className={cx(COL.dir, "font-mono text-[11px] text-txt-muted truncate")} title={dirOf(file.relative_path)}>
          {dirOf(file.relative_path) || "/"}
        </span>
      )}
      <span className={cx(COL.size, "font-mono text-[11px] text-txt-dim tabular")}>{formatBytes(file.size)}</span>
      <span className={cx(COL.modified, "font-mono text-[11px] text-txt-muted tabular")} title={file.modified ? formatDate(file.modified) : undefined}>
        {formatRelative(file.modified)}
      </span>
      <div className={cx(COL.tags, "flex items-center justify-end gap-1.5 min-w-0")}>
        <span className="min-w-0 truncate font-mono text-[10px] uppercase tracking-[0.06em] text-accent-light" title={tags.join(", ")}>
          {tags.slice(0, 2).map((t) => `#${t}`).join(" ")}
          {tags.length > 2 && <span className="text-txt-muted"> +{tags.length - 2}</span>}
        </span>
        {onTagsChanged && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              setShowTagEditor(!showTagEditor);
            }}
            className={cx(
              "w-6 h-6 shrink-0 grid place-items-center border transition-colors",
              showTagEditor
                ? "border-neon text-neon"
                : "border-transparent text-txt-muted opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:border-line-hi hover:text-txt",
            )}
            title="Edit tags"
            aria-label={`Edit tags for ${name}`}
          >
            <Tag size={12} />
          </button>
        )}
      </div>
      <div className={COL.status}>
        <StatusBadge status={syncStatus} />
      </div>
      {canToggle && (
        // The switch sits inside a clickable row: keep its clicks from opening details.
        <div className={COL.toggle} onClick={(e) => e.stopPropagation()} title={isDisabled ? "Disabled: click to enable" : "Enabled: click to disable"}>
          <Toggle checked={!isDisabled} disabled={toggleBusy} onChange={(on) => onToggle?.(file.relative_path, on)} />
        </div>
      )}
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

export default memo(ModItem);
