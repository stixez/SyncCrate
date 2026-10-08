import { X, FolderOpen, Puzzle, Palette, Power, PowerOff, AlertTriangle, Copy, Link2 } from "lucide-react";
import { useDialog } from "../hooks/useDialog";
import { friendlyError } from "../lib/errors";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { FileInfo, ModCompatibility, ModMeta, ModUpdate, SourceLink } from "../lib/types";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { MOD_SOURCE_LABELS, useModIcon } from "../lib/modMeta";
import { ModIcon } from "./ModItem";
import { dirOf, displayPath, formatBytes, formatDate, isDisabledPath, modLabel, renameInManifest } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";
import StatusBadge from "./StatusBadge";
import FileHistory from "./FileHistory";
import { Badge, Banner, Button, Input, cx } from "./ui";

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
  /** Offer "Share as a link" (mod-like content only). */
  canLink?: boolean;
  /** The content type's folder (game-relative), to tell a mod in a subfolder from a loose one. */
  contentFolder?: string;
  /** The link covering this file, if it's shared as one. */
  link?: SourceLink;
  onLinksChanged?: (links: SourceLink[]) => void;
  onClose: () => void;
}

/** The mod's image at a size you can see: Sims 4 thumbnails are swatches and
 * item renders that are unreadable at icon size. Nothing while loading. */
function Preview({ meta }: { meta: ModMeta }) {
  const url = useModIcon(meta);
  if (!url) return null;
  return (
    <div className="mb-2 grid place-items-center border border-border bg-bg">
      <img src={url} alt={`Preview of ${meta.name}`} className="max-h-48 max-w-full object-contain" draggable={false} />
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
  canLink,
  contentFolder,
  link,
  onLinksChanged,
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

  // "Share as a link": the creator forbids re-uploads, so friends get their page instead.
  const path = file.relative_path;
  const linkIsFolder = !!link && link.prefix.split("/").length < path.split("/").length;
  const parent = dirOf(path);
  const ctFolder = (contentFolder ?? "").replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  // Only a mod in its own subfolder: the content folder itself would cover every mod.
  const inSubfolder = !!parent && (ctFolder === "" || ctFolder === "." || parent.toLowerCase().startsWith(`${ctFolder}/`));
  const folderPrefix = linkIsFolder ? link!.prefix : parent;
  const website = meta?.website && /^https:\/\//i.test(meta.website) ? meta.website : "";
  const [linkUrl, setLinkUrl] = useState(link?.url ?? website);
  const [linkScope, setLinkScope] = useState<"file" | "folder">(linkIsFolder ? "folder" : "file");
  const [savingLink, setSavingLink] = useState(false);
  const saveLink = async () => {
    const prefix = linkScope === "folder" ? folderPrefix : path;
    setSavingLink(true);
    try {
      let links = await cmd.setSourceLink(gameId, prefix, linkUrl.trim(), meta?.name ?? null);
      // A file link becoming a folder link is replaced. The other way round
      // the folder's link stays: removing it would start copying its other mods.
      if (link && !linkIsFolder && link.prefix.toLowerCase() !== prefix.toLowerCase()) links = await cmd.removeSourceLink(gameId, link.prefix);
      onLinksChanged?.(links);
      toastSuccess(linkScope === "folder" ? "Friends get the link for this folder instead of a copy" : "Friends get the link instead of a copy");
    } catch (e) {
      toastError(friendlyError(e));
    } finally {
      setSavingLink(false);
    }
  };
  const removeLink = async () => {
    if (!link) return;
    setSavingLink(true);
    try {
      onLinksChanged?.(await cmd.removeSourceLink(gameId, link.prefix));
      toastSuccess(linkIsFolder ? "Link removed for the whole folder" : "Link removed");
    } catch (e) {
      toastError(friendlyError(e));
    } finally {
      setSavingLink(false);
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
        {/* Only the middle scrolls: a mod with a long description and history
            pushed Close and the buttons off a small window. */}
        <div className="panel shadow-2xl flex flex-col max-h-[calc(var(--app-h)-4rem)]">
          <div className="shrink-0 flex items-start justify-between gap-3 px-5 pt-5 pb-4 border-b border-border">
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
                <p className="hud-label"><b>//</b> {meta ? MOD_SOURCE_LABELS[meta.source] ?? "Mod" : isMod ? modLabel(file.relative_path, gameId) : "Custom content"}</p>
                <h3 id="mod-details-title" className="font-display font-semibold uppercase tracking-[0.04em] text-[15px] leading-tight truncate" title={meta?.name ?? name}>
                  {meta?.name ?? name}
                </h3>
                {meta && !(meta.is_file && meta.derived_name) && <p className="font-mono text-[11px] text-txt-muted truncate" title={name}>{name}</p>}
              </div>
            </div>
            <button ref={closeRef} onClick={onClose} className="p-1 text-txt-muted hover:text-txt transition-colors" aria-label="Close">
              <X size={18} />
            </button>
          </div>

          <div className="min-h-0 overflow-y-auto">
            <div className="px-5 py-3">
              {meta?.has_icon && <Preview meta={meta} />}
              {meta && (
                <>
                  {meta.version && <Row label="Version"><span className="font-mono text-xs">{meta.version}</span></Row>}
                  {meta.items != null && meta.items > 1 && <Row label="Contains">{meta.items} items</Row>}
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
                  {link && <Badge tone="neutral" title="Friends get a link to the creator's page instead of a copy">Link</Badge>}
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

            {canLink && (
              <div className="px-5 pb-3">
                <p className="hud-label mb-1.5">// Share as a link</p>
                <p className="text-xs text-txt-dim mb-2">For mods the creator asks you not to re-upload. Friends get this link instead of a copy.</p>
                <Input
                  size="sm"
                  mono
                  value={linkUrl}
                  onChange={(e) => setLinkUrl(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && linkUrl.trim() && !savingLink && saveLink()}
                  placeholder="https://"
                  aria-label="Link to the creator's page"
                  icon={<Link2 size={12} />}
                />
                {(inSubfolder || linkIsFolder) && (
                  <div className="flex mt-2" role="radiogroup" aria-label="What the link covers">
                    {(["file", "folder"] as const).map((scope, i) => (
                      <button
                        key={scope}
                        role="radio"
                        aria-checked={linkScope === scope}
                        onClick={() => setLinkScope(scope)}
                        className={cx(
                          "h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border transition-colors",
                          i > 0 && "-ml-px",
                          linkScope === scope ? "relative z-1 bg-neon/10 border-neon text-neon" : "bg-bg border-line-hi text-txt-dim hover:text-txt",
                        )}
                      >
                        {scope === "file" ? "This file" : "Whole folder"}
                      </button>
                    ))}
                  </div>
                )}
                {linkScope === "folder" && (
                  <p className="font-mono text-[10.5px] text-txt-muted mt-1 truncate" title={folderPrefix}>Every file in {folderPrefix}/</p>
                )}
                <div className="flex gap-2 mt-2">
                  <Button size="sm" variant="primary" onClick={saveLink} disabled={savingLink || !linkUrl.trim()}>
                    Save
                  </Button>
                  {link && (
                    <Button size="sm" variant="ghost" onClick={removeLink} disabled={savingLink}>
                      Remove
                    </Button>
                  )}
                </div>
              </div>
            )}

            <div className="px-5 pb-3">
              <p className="hud-label mb-1.5">// Earlier versions</p>
              <FileHistory gameId={gameId} path={file.relative_path} className="max-h-48 overflow-y-auto" />
            </div>

            {compatibility?.status === "MissingPacks" && (
              <Banner tone="warn" icon={<AlertTriangle size={14} />} title="Missing packs" className="mx-5 mb-3">
                <span className="font-mono">{compatibility.missing_packs.map((p) => p.code).join(", ")}</span>
              </Banner>
            )}
          </div>

          <div className="shrink-0 flex gap-2 px-5 pb-5 pt-1">
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
