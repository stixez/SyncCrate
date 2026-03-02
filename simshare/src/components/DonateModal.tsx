import { X, Coffee, Heart, ExternalLink } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { open } from "@tauri-apps/plugin-shell";
import { getSyncCount, getTimeSaved } from "../lib/donations";

export default function DonateModal() {
  const setShowDonate = useAppStore((s) => s.setShowDonate);
  const syncCount = getSyncCount();

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={() => setShowDonate(false)} role="dialog" aria-modal="true" aria-label="Support SimShare">
      <div className="bg-bg-card rounded-2xl border border-border p-6 max-w-sm w-full mx-4" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Heart size={18} className="text-pink-400" />
            <h3 className="font-bold text-lg">Support SimShare</h3>
          </div>
          <button
            onClick={() => setShowDonate(false)}
            className="p-1 rounded-lg hover:bg-bg-card-hover transition-colors"
            aria-label="Close"
          >
            <X size={18} className="text-txt-dim" />
          </button>
        </div>

        <p className="text-sm text-txt-dim mb-2">
          SimShare is free, open-source, and ad-free forever.
        </p>
        <p className="text-sm text-txt-dim mb-5">
          One coffee funds 2 hours of development.
        </p>

        {syncCount > 0 && (
          <div className="bg-bg rounded-lg px-3 py-2 mb-4 text-center">
            <p className="text-xs text-txt-dim">
              <span className="text-accent-light font-semibold">{syncCount}</span> sync{syncCount !== 1 ? "s" : ""} — that's <span className="text-accent-light font-semibold">{getTimeSaved(syncCount)}</span> you didn't spend copying files
            </p>
          </div>
        )}

        <div className="space-y-3">
          <button
            onClick={() => open("https://www.buymeacoffee.com/stixe").catch(() => {})}
            className="w-full flex items-center gap-3 bg-status-yellow/10 hover:bg-status-yellow/20 border border-status-yellow/30 rounded-xl px-4 py-3 transition-colors text-left group"
          >
            <Coffee size={20} className="text-status-yellow" />
            <div className="flex-1">
              <p className="text-sm font-medium">Buy Me a Coffee</p>
              <p className="text-xs text-txt-dim">One-time support</p>
            </div>
            <ExternalLink size={14} className="text-txt-dim opacity-0 group-hover:opacity-100 transition-opacity" />
          </button>
        </div>
      </div>
    </div>
  );
}
