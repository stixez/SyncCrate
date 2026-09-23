import type { ReactNode } from "react";
import { cx } from "./cx";

export interface StatTileProps {
  value: ReactNode;
  label: ReactNode;
  /** Small line under the label (unit, delta). */
  hint?: ReactNode;
  icon?: ReactNode;
  /** Neon left rule + neon value — for the one number that matters most. */
  highlight?: boolean;
  /** Smaller number and padding, for summary strips above dense tables. */
  compact?: boolean;
  className?: string;
}

/** Big display number with a mono caption and a left rule (website "stat" style). */
export default function StatTile({ value, label, hint, icon, highlight, compact, className }: StatTileProps) {
  return (
    <div
      className={cx(
        "relative bg-bg-card border border-border pl-4 pr-3 min-w-0",
        compact ? "py-2" : "py-3",
        "before:absolute before:left-[-1px] before:top-[-1px] before:bottom-[-1px] before:w-[2px]",
        highlight ? "before:bg-neon" : "before:bg-line-hi",
        className,
      )}
    >
      <div className={cx("flex items-center justify-between gap-2", compact ? "mb-1" : "mb-2")}>
        <span className="hud-label truncate">{label}</span>
        {icon && <span className="text-txt-muted shrink-0">{icon}</span>}
      </div>
      <p className={cx("font-display font-bold leading-none tabular truncate", compact ? "text-[1.3rem]" : "text-[1.9rem]", highlight ? "text-neon" : "text-txt")}>
        {value}
      </p>
      {hint && <p className="text-[11px] text-txt-muted mt-1.5 truncate">{hint}</p>}
    </div>
  );
}
