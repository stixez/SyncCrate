import { Badge } from "./ui";
import type { BadgeTone } from "./ui/Badge";

interface StatusBadgeProps {
  status: "synced" | "pending" | "conflict" | "local";
}

const tooltips: Record<string, string> = {
  synced: "Same as the host's copy",
  pending: "This file will be synced in the next transfer",
  conflict: "Different versions exist — resolve before syncing",
  local: "Only on this PC (the host doesn't have it)",
};

const config: Record<StatusBadgeProps["status"], { label: string; tone: BadgeTone }> = {
  synced: { label: "Synced", tone: "green" },
  pending: { label: "To sync", tone: "amber" },
  conflict: { label: "Conflict", tone: "red" },
  local: { label: "Local only", tone: "neutral" },
};

export default function StatusBadge({ status }: StatusBadgeProps) {
  const c = config[status];
  return (
    <Badge tone={c.tone} dot title={tooltips[status]}>
      {c.label}
    </Badge>
  );
}
