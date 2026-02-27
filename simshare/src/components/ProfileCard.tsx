import { Download, Trash2, Play } from "lucide-react";
import type { ModProfile } from "../lib/types";
import { formatDateShort } from "../lib/utils";

interface ProfileCardProps {
  profile: ModProfile;
  onDelete: () => void;
  onLoad: () => void;
  onExport: () => void;
  isDeletePending?: boolean;
  onCancelDelete?: () => void;
}

export default function ProfileCard({ profile, onDelete, onLoad, onExport, isDeletePending, onCancelDelete }: ProfileCardProps) {
  return (
    <div className="bg-bg-card rounded-xl border border-border p-4 hover:border-accent/30 transition-colors">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2">
          <span className="text-xl">{profile.icon}</span>
          <div>
            <h3 className="font-semibold text-sm">{profile.name}</h3>
            <p className="text-xs text-txt-dim">{profile.author}</p>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
            profile.game === "Sims2"
              ? "bg-blue-500/20 text-blue-400"
              : profile.game === "Sims3"
              ? "bg-emerald-500/20 text-emerald-400"
              : "bg-accent/20 text-accent-light"
          }`}>
            {profile.game === "Sims2" ? "Sims 2" : profile.game === "Sims3" ? "Sims 3" : "Sims 4"}
          </span>
          <span className="text-xs text-txt-dim">{formatDateShort(profile.created_at)}</span>
        </div>
      </div>
      {profile.description && (
        <p className="text-xs text-txt-dim mb-3 line-clamp-2">{profile.description}</p>
      )}
      <p className="text-xs text-txt-dim mb-3">{profile.mods.length} mods</p>
      {isDeletePending ? (
        <div className="flex gap-2 items-center">
          <span className="text-xs text-status-red flex-1">Delete this profile?</span>
          <button
            onClick={onDelete}
            className="flex items-center justify-center gap-1.5 bg-status-red hover:bg-status-red/80 text-white rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
          >
            Confirm
          </button>
          <button
            onClick={onCancelDelete}
            className="flex items-center justify-center gap-1.5 bg-bg-card-hover hover:bg-bg-card-active rounded-lg px-3 py-1.5 text-xs text-txt-dim transition-colors"
          >
            Cancel
          </button>
        </div>
      ) : (
        <div className="flex gap-2">
          <button
            onClick={onLoad}
            className="flex-1 flex items-center justify-center gap-1.5 bg-accent/20 hover:bg-accent/30 text-accent-light rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
          >
            <Play size={12} />
            Load
          </button>
          <button
            onClick={onExport}
            className="flex items-center justify-center gap-1.5 bg-bg-card-hover hover:bg-bg-card-active rounded-lg px-3 py-1.5 text-xs text-txt-dim transition-colors"
          >
            <Download size={12} />
          </button>
          <button
            onClick={onDelete}
            className="flex items-center justify-center gap-1.5 bg-status-red/10 hover:bg-status-red/20 text-status-red rounded-lg px-3 py-1.5 text-xs transition-colors"
          >
            <Trash2 size={12} />
          </button>
        </div>
      )}
    </div>
  );
}
