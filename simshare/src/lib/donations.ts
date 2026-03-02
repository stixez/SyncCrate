const STORAGE_KEY = "simshare-sync-count";
const DISMISSED_KEY = "simshare-donation-dismissed";

const MILESTONES = [10, 50, 100] as const;

export function getSyncCount(): number {
  return parseInt(localStorage.getItem(STORAGE_KEY) || "0", 10);
}

export function incrementSyncCount(): number {
  const count = getSyncCount() + 1;
  localStorage.setItem(STORAGE_KEY, String(count));
  return count;
}

export function getDismissedMilestones(): number[] {
  try {
    return JSON.parse(localStorage.getItem(DISMISSED_KEY) || "[]");
  } catch {
    return [];
  }
}

export function dismissMilestone(milestone: number) {
  const dismissed = getDismissedMilestones();
  if (!dismissed.includes(milestone)) {
    dismissed.push(milestone);
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(dismissed));
  }
}

/** Returns a milestone number if one should be shown, or null */
export function checkMilestone(syncCount: number): number | null {
  const dismissed = getDismissedMilestones();
  // Find the highest milestone reached that hasn't been dismissed
  for (let i = MILESTONES.length - 1; i >= 0; i--) {
    if (syncCount >= MILESTONES[i] && !dismissed.includes(MILESTONES[i])) {
      return MILESTONES[i];
    }
  }
  return null;
}

/** Estimate time saved: ~15 min per sync for manual copy/compare */
export function getTimeSaved(syncCount: number): string {
  const minutes = syncCount * 15;
  if (minutes < 60) return `${minutes} minutes`;
  const hours = Math.round(minutes / 60);
  return `~${hours} hour${hours !== 1 ? "s" : ""}`;
}

export function getMilestoneMessage(milestone: number): { title: string; message: string } {
  const timeSaved = getTimeSaved(milestone);
  switch (milestone) {
    case 10:
      return {
        title: "Thanks for using SimShare!",
        message: `You've synced 10 times, saving yourself ${timeSaved} of manual work. SimShare is free forever — donations help fund future updates.`,
      };
    case 50:
      return {
        title: "You're a power user!",
        message: `50 syncs and ${timeSaved} saved! If SimShare has been useful, consider chipping in.`,
      };
    case 100:
      return {
        title: "100 syncs!",
        message: `That's ${timeSaved} you didn't spend copying files manually. A one-time coffee helps keep development going.`,
      };
    default:
      return {
        title: "Thanks for using SimShare!",
        message: "SimShare is free forever. Donations help fund future updates.",
      };
  }
}
