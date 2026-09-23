import { cx } from "./cx";

/** Pulsing status dot. neon = live/connected, amber = connecting/attention, idle = off. */
export default function LiveDot({ tone = "neon", className }: { tone?: "neon" | "amber" | "idle"; className?: string }) {
  return <span aria-hidden="true" className={cx("live-dot", tone === "amber" ? "live-dot-amber" : tone === "idle" ? "live-dot-idle" : "", className)} />;
}
