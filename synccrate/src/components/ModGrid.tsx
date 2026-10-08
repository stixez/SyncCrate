import { memo, useEffect, useMemo, useState } from "react";
import { Puzzle, Palette, AlertTriangle, CheckSquare, Square } from "lucide-react";
import type { FileInfo, ModCompatibility, ModMeta, ModUpdate, ModWarning } from "../lib/types";
import { useModIcon } from "../lib/modMeta";
import { useVirtualList } from "../hooks/useVirtualList";
import { fileName, formatBytes, isDisabledPath, plural } from "../lib/utils";
import StatusBadge from "./StatusBadge";
import { Badge, cx } from "./ui";

type SyncStatus = "synced" | "pending" | "conflict" | "local";

/** One tile: a single-file mod, or every file of a folder mod with metadata. */
export interface GridUnit {
  key: string;
  files: FileInfo[];
  meta?: ModMeta;
}

// Tiles are at least this wide; the column count follows the box width.
const TILE_MIN = 136;
const GAP = 10;
const PAD = 10;
// Name (two lines) + size/status line under the square picture.
const CAPTION_H = 58;

const STATUS_RANK: Record<SyncStatus, number> = { conflict: 3, pending: 2, local: 1, synced: 0 };

interface GridProps {
  units: GridUnit[];
  getSyncStatus: (path: string) => SyncStatus;
  updateFor: (key?: string) => ModUpdate | undefined;
  warningsFor: (key?: string) => ModWarning[] | undefined;
  outdatedPaths: Set<string>;
  compatMap: Map<string, ModCompatibility>;
  bulkMode: boolean;
  selected: Set<string>;
  onSelectPaths: (paths: string[], on: boolean) => void;
  onShowDetails: (file: FileInfo) => void;
  /** Shared as a link to its creator instead of being copied to friends. */
  isLinked: (path: string) => boolean;
}

/**
 * Picture grid for the Content page: the same files as the list, as tiles
 * with each mod's thumbnail (Sims 4 CC, Thunderstore icons, Minecraft
 * logos…). Virtualised by row against the page scroll like the list, so a
 * 20k-file Mods folder stays smooth; only visible tiles load their image.
 */
export default function ModGrid({ units, ...rest }: GridProps) {
  const [width, setWidth] = useState(0);
  const cols = Math.max(1, Math.floor((width - 2 * PAD + GAP) / (TILE_MIN + GAP)));
  const tileW = width ? (width - 2 * PAD - (cols - 1) * GAP) / cols : TILE_MIN;
  const rowH = Math.round(tileW + CAPTION_H + GAP);
  const rows = useMemo(() => {
    const out: GridUnit[][] = [];
    for (let i = 0; i < units.length; i += cols) out.push(units.slice(i, i + cols));
    return out;
  }, [units, cols]);
  const heights = useMemo(() => rows.map(() => rowH), [rows, rowH]);
  const { ref, offsets, total, start, end } = useVirtualList(heights);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, [ref]);

  const visibleRows = [];
  for (let i = start; i < end && i < rows.length; i++) {
    visibleRows.push(
      <div
        key={i}
        className="absolute inset-x-0 grid"
        style={{ top: offsets[i] + PAD, gap: GAP, paddingInline: PAD, gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
      >
        {rows[i].map((u) => (
          <Tile key={u.key} unit={u} size={tileW} {...rest} />
        ))}
      </div>,
    );
  }
  return (
    <div ref={ref} className="relative" style={{ height: total + PAD }}>
      {visibleRows}
    </div>
  );
}

const Tile = memo(function Tile({
  unit,
  size,
  getSyncStatus,
  updateFor,
  warningsFor,
  outdatedPaths,
  compatMap,
  bulkMode,
  selected,
  onSelectPaths,
  onShowDetails,
  isLinked,
}: { unit: GridUnit; size: number } & Omit<GridProps, "units">) {
  const { files, meta } = unit;
  const first = files[0];
  const paths = files.map((f) => f.relative_path);
  const url = useModIcon(meta);
  const name = meta?.name ?? fileName(first.relative_path).replace(/\.disabled$/i, "");
  const isMod = first.file_type === "Mod";
  const disabled = paths.every(isDisabledPath);
  const status = paths.map(getSyncStatus).reduce((a, b) => (STATUS_RANK[b] > STATUS_RANK[a] ? b : a), "synced" as SyncStatus);
  const update = updateFor(meta?.key);
  const warnings = warningsFor(meta?.key);
  const outdated = paths.some((p) => outdatedPaths.has(p));
  const linked = paths.some(isLinked);
  const missingPacks = paths.some((p) => compatMap.get(p)?.status === "MissingPacks");
  const isSelected = bulkMode && paths.every((p) => selected.has(p));
  const bytes = files.reduce((n, f) => n + f.size, 0);
  const activate = () => (bulkMode ? onSelectPaths(paths, !isSelected) : onShowDetails(first));

  return (
    <button
      type="button"
      onClick={activate}
      title={files.length > 1 ? `${name} · ${plural(files.length, "file")}` : `${name} · ${first.relative_path}`}
      aria-pressed={bulkMode ? isSelected : undefined}
      className={cx(
        "group relative flex flex-col text-left border bg-bg-2 transition-colors min-w-0",
        isSelected ? "border-neon bg-neon/6" : outdated ? "border-amber/60 hover:border-amber" : "border-border hover:border-neon/60",
      )}
    >
      <span
        className={cx("relative grid place-items-center overflow-hidden bg-bg border-b border-border", disabled && "opacity-45 grayscale")}
        style={{ height: size }}
      >
        {url ? (
          <img src={url} alt="" className="w-full h-full object-contain" draggable={false} loading="lazy" />
        ) : (
          <span className={cx("grid place-items-center w-14 h-14 border", isMod ? "border-accent/50 text-accent-light bg-accent/10" : "border-line-hi text-txt-dim")}>
            {isMod ? <Puzzle size={24} /> : <Palette size={24} />}
          </span>
        )}
        {bulkMode && (
          <span className={cx("absolute top-1.5 left-1.5", isSelected ? "text-neon" : "text-txt-muted")} aria-hidden="true">
            {isSelected ? <CheckSquare size={16} /> : <Square size={16} />}
          </span>
        )}
        <span className="absolute top-1.5 right-1.5 flex flex-col items-end gap-1">
          {disabled && <Badge tone="neutral">Off</Badge>}
          {update && <Badge tone="neon" title={`Update available: ${update.latest}`}>Update</Badge>}
          {warnings && (
            <Badge tone="amber" title={warnings.map((w) => w.text).join("\n")}>
              {warnings.some((w) => w.kind === "missing_dependency") ? "Needs mod" : "Mismatch"}
            </Badge>
          )}
          {outdated && <Badge tone="amber">Outdated</Badge>}
          {linked && <Badge tone="neutral" title="Friends get a link to the creator's page instead of a copy">Link</Badge>}
          {meta?.items != null && meta.items > 1 && <Badge tone="neutral" title="Items in this merged package">{meta.items}</Badge>}
        </span>
        {missingPacks && (
          <span className="absolute bottom-1.5 right-1.5 text-amber" title="Needs packs you don't have">
            <AlertTriangle size={14} />
          </span>
        )}
      </span>
      <span className="flex-1 flex flex-col justify-between gap-1 px-2 py-1.5 min-w-0">
        <span className={cx("text-[12px] leading-tight font-medium line-clamp-2 break-words", disabled ? "text-txt-muted" : "text-txt")}>{name}</span>
        <span className="flex items-center justify-between gap-2 font-mono text-[10px] text-txt-muted tabular">
          <span className="truncate">{files.length > 1 ? plural(files.length, "file") : formatBytes(bytes)}</span>
          {status !== "synced" && status !== "local" && <StatusBadge status={status} />}
        </span>
      </span>
    </button>
  );
});
