import { X, Coffee, Heart } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { dismissMilestone, getMilestoneMessage } from "../lib/donations";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "./ui";

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
    <div className="panel panel-accent mb-4">
      <div className="px-5 py-4 flex items-start gap-4">
        <div className="w-10 h-10 grid place-items-center border border-neon/40 bg-neon/10 shrink-0">
          <Heart size={17} className="text-neon" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="hud-label mb-1"><b>//</b> Milestone</p>
          <h4 className="font-display font-semibold uppercase tracking-[0.05em] text-[0.95rem] leading-tight">{title}</h4>
          <p className="text-xs text-txt-dim mt-1.5">{message}</p>
          <div className="flex items-center gap-2 mt-3">
            <Button variant="primary" size="sm" onClick={handleDonate} icon={<Coffee size={12} />}>
              Buy a Coffee
            </Button>
            <Button variant="ghost" size="sm" onClick={handleDismiss}>
              Maybe later
            </Button>
          </div>
        </div>
        <button
          onClick={handleDismiss}
          className="w-7 h-7 grid place-items-center text-txt-dim hover:text-txt hover:bg-bg-card-hover transition-colors shrink-0"
          aria-label="Dismiss"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
