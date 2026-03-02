import { X, Coffee, Heart } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { dismissMilestone, getMilestoneMessage } from "../lib/donations";
import { open } from "@tauri-apps/plugin-shell";

export default function DonationBanner() {
  const milestone = useAppStore((s) => s.donationMilestone);
  const setDonationMilestone = useAppStore((s) => s.setDonationMilestone);

  if (!milestone) return null;

  const { title, message } = getMilestoneMessage(milestone);

  const handleDismiss = () => {
    dismissMilestone(milestone);
    setDonationMilestone(null);
  };

  const handleDonate = () => {
    open("https://www.buymeacoffee.com/stixe").catch(() => {});
    handleDismiss();
  };

  return (
    <div className="bg-bg-card border border-accent/30 rounded-xl p-4 mb-4">
      <div className="flex items-start gap-3">
        <div className="p-2 rounded-lg bg-accent/10 shrink-0">
          <Heart size={18} className="text-accent-light" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between gap-2">
            <h4 className="font-semibold text-sm">{title}</h4>
            <button
              onClick={handleDismiss}
              className="p-1 rounded-lg hover:bg-bg-card-hover transition-colors shrink-0"
              aria-label="Dismiss"
            >
              <X size={14} className="text-txt-dim" />
            </button>
          </div>
          <p className="text-xs text-txt-dim mt-1">{message}</p>
          <div className="flex items-center gap-2 mt-3">
            <button
              onClick={handleDonate}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-xs font-medium transition-colors"
            >
              <Coffee size={12} />
              Buy a Coffee
            </button>
            <button
              onClick={handleDismiss}
              className="px-3 py-1.5 rounded-lg text-xs text-txt-dim hover:bg-bg-card-hover transition-colors"
            >
              Maybe later
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
