import { X, FolderOpen, Puzzle, Palette, Power, PowerOff, AlertTriangle, Copy } from "lucide-react";
import { useDialog } from "../hooks/useDialog";
import { friendlyError } from "../lib/errors";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { FileInfo, ModCompatibility, ModMeta, ModUpdate } from "../lib/types";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { MOD_SOURCE_LABELS, useModIcon } from "../lib/modMeta";
import { ModIcon } from "./ModItem";
import { displayPath, formatBytes, formatDate, isDisabledPath, renameInManifest } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";
import StatusBadge from "./StatusBadge";
import FileHistory from "./FileHistory";
import { Badge, Banner, Button, cx } from "./ui";

interface ModDetailsPanelProps {
  /** The game whose content page opened this (not necessarily the active game). */
  gameId: string;
  /** False when toggling isn't possible (other content type, read-only, "none" games). */
  canToggle: boolean;
  file: FileInfo;
  meta?: ModMeta;
  update?: ModUpdate;
  syncStatus: "synced" | "pending" | "conflict" | "local";
  tags: string[];
  compatibility?: ModCompatibility;
  onClose: () => void;
}

/** The mod's image at a size you can see: Sims 4 thumbnails are swatches and
 * item renders that are unreadable at icon size. Nothing while loading. */
function Preview({ meta }: { meta: ModMeta }) {
  const url = useModIcon(meta);
  if (!url) return null;
  return (
    <div className="mb-2 grid place-items-center border border-border bg-bg">
      <img src={url} alt="" className="max-h-48 max-w-full object-contain" draggable={false} />
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[88px_1fr] gap-3 py-2 border-b border-border last:border-b-0 items-start">
      <span className="hud-label pt-px">{label}</span>
      <div className="min-w-0 text-[13px] text-txt">{children}</div>
    </div>
  );
}

export default function ModDetailsPanel({
  gameId,
  canToggle,
  file,
  meta,
  update,
  syncStatus,
  tags,
  compatibility,
  onClose,
}: ModDetailsPanelProps) {
  const gamePaths = useAppStore((s) => s.gamePaths);
  const setManifest = useAppStore((s) => s.setManifest);
  const setModTags = useAppStore((s) => s.setModTags);
  const isMod = file.file_type === "Mod";
  const name = file.relative_path.split(/[/\\]/).pop() || file.relative_path;
  const isDisabled = isDisabledPath(file.relative_path);
  const basePath = gamePaths[gameId];

  // A double click ran a second toggle on the old (already renamed) path.
  const [toggling, setToggling] = useState(false);
  const handleToggle = async () => {
    if (toggling) return;
    setToggling(true);
    try {
      const newPath = await cmd.toggleMod(gameId, file.relative_path, isDisabled);
      const m = useAppStore.getState().manifest;
      if (m) setManifest(renameInManifest(m, [[file.relative_path, newPath]]));
      cmd.getModTags(gameId).then(setModTags).catch(() => {});
      toastSuccess(isDisabled ? `Enabled ${name}` : `Disabled ${name}`);
      // The file now has a new path; this panel still points at the old one.
      onClose();
    } catch (e) {
      toastError(friendlyError(e));
    } finally {
      setToggling(false);
    }
  };

  // The shared dialog behaviour: Escape (topmost dialog only), focus kept
  // inside, and back on the row it was opened from (it fell to the page,
  // losing the place in a list of thousands of files).
  const closeRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useDialog(onClose);

  // A `@<id>/` path lives outside the game folder (Valheim worlds): ask
  // where; gluing it onto the game folder named a folder that isn't there.
  const outside = file.relative_path.startsWith("@");
  const [resolved, setResolved] = useState("");
  useEffect(() => {
    if (!outside) return;
    let live = true;
    cmd.resolveContentPath(file.relative_path).then((p) => live && setResolved(p)).catch(() => live && setResolved(""));
    return () => { live = false; };
  }, [outside, file.relative_path]);
  const fullPath = outside ? resolved : basePath ? `${basePath}/${file.relative_path}` : "";
  const handleReveal = () => {
    if (!fullPath) return;
    cmd.revealFile(fullPath).catch((e) => toastError(`Couldn't show the file: ${e}`));
  };
  const copyPath = () => {
    navigator.clipboard.writeText(fullPath.replace(/\//g, "\\")).then(
      () => toastSuccess("Path copied"),
      () => toastError("Couldn't copy to the clipboard."),
    );
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/80 backdrop-blur-[2px]" onClick={onClose}>
      <div ref={dialogRef} className="corner-brackets w-full max-w-md mx-4" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="mod-details-title">
        <div className="panel shadow-2xl">
          <div className="flex items-start justify-between gap-3 px-5 pt-5 pb-4 border-b border-border">
            <div className="flex items-center gap-3 min-w-0">
              <div
                className={cx(
                  "w-10 h-10 shrink-0 grid place-items-center border",
                  isMod ? "border-accent/50 text-accent-light bg-accent/10" : "border-line-hi text-txt-dim bg-bg",
                )}
              >
                <ModIcon meta={meta} size={38} fallback={isMod ? <Puzzle size={18} /> : <Palette size={18} />} />
              </div>
              <div className="min-w-0">
                <p className="hud-label"><b>//</b> {meta ? MOD_SOURCE_LABELS[meta.source] ?? "Mod" : isMod ? "Script mod" : "Custom content"}</p>
                <h3 id="mod-details-title" className="font-display font-semibold uppercase tracking-[0.04em] text-[15px] leading-tight truncate" title={meta?.name ?? name}>
                  {meta?.name ?? name}
                </h3>
                {meta && !meta.is_file && <p className="font-mono text-[11px] text-txt-muted truncate" title={name}>{name}</p>}
              </div>
            </div>
            <button ref={closeRef} onClick={onClose} className="p-1 text-txt-muted hover:text-txt transition-colors" aria-label="Close">
              <X size={18} />
            </button>
          </div>

          <div className="px-5 py-3">
            {meta?.has_icon && <Preview meta={meta} />}
            {meta && (
              <>
                {meta.version && <Row label="Version"><span className="font-mono text-xs">{meta.version}</span></Row>}
                {meta.items != null && meta.items > 1 && <Row label="Items"><span className="font-mono text-xs tabular">{meta.items}</span></Row>}
                {update && (
                  <Row label="Update">
                    <span className="font-mono text-xs text-neon">{update.latest}</span>
                    {update.deprecated && <span className="text-xs text-amber ml-2">no longer maintained</span>}
                    {update.url && (
                      <button className="ml-2 font-mono text-[11px] text-accent-light hover:text-neon" onClick={() => openUrl(update.url!).catch(() => {})}>
                        Get it
                      </button>
                    )}
                  </Row>
                )}
                {meta.authors.length > 0 && <Row label={meta.authors.length > 1 ? "Authors" : "Author"}>{meta.authors.join(", ")}</Row>}
                {meta.description && (
                  <Row label="About">
                    <p className="text-xs text-txt-dim whitespace-pre-line max-h-28 overflow-y-auto">{meta.description}</p>
                  </Row>
                )}
                {meta.website && (
                  <Row label="Website">
                    <button className="font-mono text-[11px] text-accent-light hover:text-neon break-all text-left" onClick={() => openUrl(meta.website!).catch(() => {})}>
                      {meta.website}
                    </button>
                  </Row>
                )}
              </>
            )}
            <Row label="Path">
              <span className="block font-mono text-[11px] text-txt-dim break-all">{outside ? resolved || displayPath(file.relative_path) : file.relative_path}</span>
            </Row>
            <Row label="Size"><span className="font-mono text-xs tabular">{formatBytes(file.size)}</span></Row>
            <Row label="Modified"><span className="font-mono text-xs">{formatDate(file.modified)}</span></Row>
            {file.hash && <Row label="Hash"><span className="block font-mono text-[11px] text-txt-dim break-all">{file.hash}</span></Row>}
            <Row label="Status">
              <div className="flex items-center gap-2">
                <StatusBadge status={syncStatus} />
                {isDisabled && <Badge tone="neutral">Disabled</Badge>}
              </div>
            </Row>
            {tags.length > 0 && (
              <Row label="Tags">
                <div className="flex flex-wrap gap-1">
                  {tags.map((tag) => (
                    <Badge key={tag} tone="neutral" className="text-accent-light">{tag}</Badge>
                  ))}
                </div>
              </Row>
            )}
          </div>

          <div className="px-5 pb-3">
            <p className="hud-label mb-1.5">// Earlier versions</p>
            <FileHistory gameId={gameId} path={file.relative_path} className="max-h-48 overflow-y-auto" />
          </div>

          {compatibility?.status === "MissingPacks" && (
            <Banner tone="warn" icon={<AlertTriangle size={14} />} title="Missing packs" className="mx-5 mb-3">
              <span className="font-mono">{compatibility.missing_packs.map((p) => p.code).join(", ")}</span>
            </Banner>
          )}

          <div className="flex gap-2 px-5 pb-5 pt-1">
            {canToggle && (
              <Button
                variant={isDisabled ? "primary" : "secondary"}
                onClick={handleToggle}
                disabled={toggling}
                icon={isDisabled ? <Power size={14} /> : <PowerOff size={14} />}
              >
                {isDisabled ? "Enable" : "Disable"}
              </Button>
            )}
            {basePath && (
              <>
                <Button variant="ghost" onClick={handleReveal} icon={<FolderOpen size={14} />}>
                  Show in folder
                </Button>
                <Button variant="ghost" onClick={copyPath} icon={<Copy size={14} />} aria-label="Copy the file's full path" title="Copy the file's full path" />
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
