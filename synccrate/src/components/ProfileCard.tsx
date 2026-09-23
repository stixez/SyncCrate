import { Download, Trash2, BarChart3 } from "lucide-react";
import type { ModProfile } from "../lib/types";
import { formatDateShort } from "../lib/utils";
import { gameLabel } from "../lib/games";
import { Badge, Button } from "./ui";

interface ProfileCardProps {
  profile: ModProfile;
  onDelete: () => void;
  onLoad: () => void;
  onExport: () => void;
  isDeletePending?: boolean;
  onCancelDelete?: () => void;
}

export default function ProfileCard({ profile, onDelete, onLoad, onExport, isDeletePending, onCancelDelete }: ProfileCardProps) {
  // Profiles store an emoji icon; the HUD look has no emoji, so show a monogram instead.
  const monogram = (profile.name.trim()[0] ?? "?").toUpperCase();

  return (
    <div
      className={
        isDeletePending
          ? "panel panel-danger flex flex-col"
          : "panel flex flex-col transition-[background] hover:[--panel-line:rgb(var(--color-neon)/0.45)]"
      }
    >
      <div className="flex items-center justify-between gap-2 px-4 pt-3 pb-2 border-b border-border">
        <p className="hud-label truncate"><b>// Loadout</b> &nbsp;{formatDateShort(profile.created_at)}</p>
        <Badge tone="neutral" className="shrink-0">{gameLabel(profile.game)}</Badge>
      </div>

      <div className="flex items-start gap-3.5 px-4 pt-4">
        <div className="w-12 h-12 shrink-0 grid place-items-center border border-accent/50 bg-accent/10 font-display font-bold text-[1.5rem] text-accent-light leading-none">
          {monogram}
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="font-display font-bold uppercase tracking-[0.03em] text-[1.05rem] leading-tight line-clamp-2 break-words" title={profile.name}>
            {profile.name}
          </h3>
          {profile.author && <p className="font-mono text-[11px] text-txt-muted mt-0.5 truncate">by {profile.author}</p>}
        </div>
        <div className="text-right shrink-0">
          <p className="font-display font-bold text-[1.7rem] leading-none tabular text-txt">{profile.mods.length}</p>
          <p className="hud-label !text-[10px] mt-1">mods</p>
        </div>
      </div>

      <p className="px-4 pt-3 text-xs text-txt-dim line-clamp-2 min-h-[2.5em]">
        {profile.description || <span className="text-txt-muted italic">No description</span>}
      </p>

      <div className="mt-auto px-4 pb-4 pt-3">
        {isDeletePending ? (
          <div className="flex gap-2 items-center">
            <span className="font-mono text-[11px] uppercase tracking-[0.08em] text-status-red flex-1">Delete this profile?</span>
            <Button size="sm" variant="danger" onClick={onDelete}>
              Confirm
            </Button>
            <Button size="sm" variant="ghost" onClick={onCancelDelete}>
              Cancel
            </Button>
          </div>
        ) : (
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" block onClick={onLoad} icon={<BarChart3 size={12} />}>
              Compare
            </Button>
            <Button size="sm" variant="ghost" onClick={onExport} aria-label="Export profile" title="Export profile" icon={<Download size={13} />} />
            <Button size="sm" variant="ghost" onClick={onDelete} aria-label="Delete profile" title="Delete profile" icon={<Trash2 size={13} />} className="hover:!text-status-red" />
          </div>
        )}
      </div>
    </div>
  );
}
