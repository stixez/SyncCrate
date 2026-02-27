interface StatusBadgeProps {
  status: "synced" | "pending" | "conflict" | "local";
}

const tooltips: Record<string, string> = {
  synced: "This file matches across all connected peers",
  pending: "This file will be synced in the next transfer",
  conflict: "Different versions exist — resolve before syncing",
  local: "This file only exists on your machine",
};

export default function StatusBadge({ status }: StatusBadgeProps) {
  const config = {
    synced: { label: "Synced", bg: "bg-status-green/20", text: "text-status-green", dot: "bg-status-green", glow: "glow-green" },
    pending: { label: "Pending", bg: "bg-status-yellow/20", text: "text-status-yellow", dot: "bg-status-yellow", glow: "glow-yellow" },
    conflict: { label: "Conflict", bg: "bg-status-red/20", text: "text-status-red", dot: "bg-status-red", glow: "glow-red" },
    local: { label: "Local Only", bg: "bg-accent/20", text: "text-accent-light", dot: "bg-accent-light", glow: "" },
  };

  const c = config[status];
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${c.bg} ${c.text} ${c.glow}`}
      title={tooltips[status]}
    >
      <span className={`w-1.5 h-1.5 rounded-full ${c.dot} mr-1.5`} />
      {c.label}
    </span>
  );
}
