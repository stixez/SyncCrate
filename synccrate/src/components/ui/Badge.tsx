import type { ReactNode } from "react";
import { cx } from "./cx";

export type BadgeTone = "neutral" | "neon" | "green" | "amber" | "red";

export interface BadgeProps {
  tone?: BadgeTone;
  icon?: ReactNode;
  /** Adds a small status dot before the text. */
  dot?: boolean;
  title?: string;
  className?: string;
  children: ReactNode;
}

const TONE: Record<BadgeTone, string> = {
  neutral: "tag-neutral text-txt-dim",
  neon: "text-neon",
  green: "text-status-green",
  amber: "text-amber",
  red: "text-status-red",
};

/** Square mono tag with a hairline border. Use for versions, counts, state — not decoration. */
export default function Badge({ tone = "neutral", icon, dot, title, className, children }: BadgeProps) {
  return (
    <span className={cx("tag", TONE[tone], className)} title={title}>
      {dot && <span className="w-1.5 h-1.5 bg-current" />}
      {icon}
      {children}
    </span>
  );
}
