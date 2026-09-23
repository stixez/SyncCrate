import { X, Coffee, Heart, ExternalLink } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { open } from "@tauri-apps/plugin-shell";
import { getSyncCount, getTimeSaved } from "../lib/donations";

export default function DonateModal() {
  const setShowDonate = useAppStore((s) => s.setShowDonate);
  const syncCount = getSyncCount();

  return (
    <div className="fixed inset-0 bg-black/65 backdrop-blur-[2px] flex items-center justify-center z-50 p-4" onClick={() => setShowDonate(false)} role="dialog" aria-modal="true" aria-label="Support SyncCrate">
      <div className="corner-brackets max-w-sm w-full" onClick={(e) => e.stopPropagation()}>
        <div className="panel shadow-2xl">
          <div className="p-6">
            <div className="flex items-start justify-between gap-3 mb-4">
              <div>
                <p className="hud-label mb-1.5 flex items-center gap-1.5">
                  <Heart size={11} className="text-neon" />
                  <span><b>//</b> Support</span>
                </p>
                <h3 className="display text-[1.6rem]">
                  Keep it <span className="text-neon">free</span>
                </h3>
              </div>
              <button
                onClick={() => setShowDonate(false)}
                className="w-8 h-8 grid place-items-center text-txt-dim hover:text-txt hover:bg-bg-card-hover transition-colors"
                aria-label="Close"
              >
                <X size={18} />
              </button>
            </div>

            <p className="text-sm text-txt-dim mb-1.5">
              SyncCrate is free, open-source, and ad-free forever.
            </p>
            <p className="text-sm text-txt-dim mb-5">
              One coffee funds 2 hours of development.
            </p>

            {syncCount > 0 && (
              <div className="grid grid-cols-2 border border-border bg-bg mb-5">
                <div className="px-3.5 py-3 border-r border-border">
                  <p className="hud-label">Syncs</p>
                  <p className="font-display font-bold text-2xl leading-none mt-1.5 text-neon tabular">{syncCount}</p>
                </div>
                <div className="px-3.5 py-3">
                  <p className="hud-label">Time saved</p>
                  <p className="font-display font-bold text-2xl leading-none mt-1.5 tabular">{getTimeSaved(syncCount)}</p>
                </div>
                <p className="col-span-2 px-3.5 py-2 border-t border-border text-[11px] text-txt-muted">
                  That's {getTimeSaved(syncCount)} you didn't spend copying files by hand.
                </p>
              </div>
            )}

            <button
              onClick={() => open("https://www.buymeacoffee.com/stixe").catch(() => {})}
              className="group w-full flex items-center gap-3 border border-amber/40 bg-amber/[0.07] hover:bg-amber/[0.14] hover:border-amber px-4 py-3 transition-colors text-left"
            >
              <div className="w-9 h-9 grid place-items-center bg-amber/15 shrink-0">
                <Coffee size={18} className="text-amber" />
              </div>
              <div className="flex-1">
                <p className="font-display font-bold uppercase tracking-[0.05em] text-sm">Buy me a coffee</p>
                <p className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-txt-muted mt-0.5">One-time support</p>
              </div>
              <ExternalLink size={14} className="text-amber opacity-40 group-hover:opacity-100 transition-opacity" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
