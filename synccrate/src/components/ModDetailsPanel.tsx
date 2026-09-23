import { X, FolderOpen, Puzzle, Palette, Power, PowerOff, AlertTriangle } from "lucide-react";
import type { ReactNode } from "react";
import type { FileInfo, ModCompatibility } from "../lib/types";
import { formatBytes, formatDate, isDisabledPath } from "../lib/utils";
import { useAppStore } from "../stores/useAppStore";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";
import StatusBadge from "./StatusBadge";
import { Badge, Banner, Button, cx } from "./ui";

interface ModDetailsPanelProps {
  file: FileInfo;
  syncStatus: "synced" | "pending" | "conflict" | "local";
  tags: string[];
  compatibility?: ModCompatibility;
  onClose: () => void;
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
  const isDisabled = isDisabledPath(file.relative_path);
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

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/80 backdrop-blur-[2px]" onClick={onClose}>
      <div className="corner-brackets w-full max-w-md mx-4" onClick={(e) => e.stopPropagation()}>
        <div className="panel shadow-2xl">
          <div className="flex items-start justify-between gap-3 px-5 pt-5 pb-4 border-b border-border">
            <div className="flex items-center gap-3 min-w-0">
              <div
                className={cx(
                  "w-10 h-10 shrink-0 grid place-items-center border",
                  isMod ? "border-accent/50 text-accent-light bg-accent/10" : "border-line-hi text-txt-dim bg-bg",
                )}
              >
                {isMod ? <Puzzle size={18} /> : <Palette size={18} />}
              </div>
              <div className="min-w-0">
                <p className="hud-label"><b>//</b> {isMod ? "Script mod" : "Custom content"}</p>
                <h3 className="font-display font-semibold uppercase tracking-[0.04em] text-[15px] leading-tight truncate" title={name}>
                  {name}
                </h3>
              </div>
            </div>
            <button onClick={onClose} className="p-1 text-txt-muted hover:text-txt transition-colors" aria-label="Close">
              <X size={18} />
            </button>
          </div>

          <div className="px-5 py-3">
            <Row label="Path">
              <span className="block font-mono text-[11px] text-txt-dim break-all">{file.relative_path}</span>
            </Row>
            <Row label="Size"><span className="font-mono text-xs tabular">{formatBytes(file.size)}</span></Row>
            <Row label="Modified"><span className="font-mono text-xs">{formatDate(file.modified)}</span></Row>
            <Row label="Hash"><span className="block font-mono text-[11px] text-txt-dim break-all">{file.hash || "N/A"}</span></Row>
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

          {compatibility?.status === "MissingPacks" && (
            <Banner tone="warn" icon={<AlertTriangle size={14} />} title="Missing packs" className="mx-5 mb-3">
              <span className="font-mono">{compatibility.missing_packs.map((p) => p.code).join(", ")}</span>
            </Banner>
          )}

          <div className="flex gap-2 px-5 pb-5 pt-1">
            <Button
              variant={isDisabled ? "primary" : "secondary"}
              onClick={handleToggle}
              icon={isDisabled ? <Power size={14} /> : <PowerOff size={14} />}
            >
              {isDisabled ? "Enable" : "Disable"}
            </Button>
            {basePath && (
              <Button variant="ghost" onClick={handleReveal} icon={<FolderOpen size={14} />}>
                Reveal in Explorer
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
