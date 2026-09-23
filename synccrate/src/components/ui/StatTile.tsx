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
  className?: string;
}

/** Big display number with a mono caption and a left rule (website "stat" style). */
export default function StatTile({ value, label, hint, icon, highlight, className }: StatTileProps) {
  return (
    <div
      className={cx(
        "relative bg-bg-card border border-border pl-4 pr-3 py-3 min-w-0",
        "before:absolute before:left-[-1px] before:top-[-1px] before:bottom-[-1px] before:w-[2px]",
        highlight ? "before:bg-neon" : "before:bg-line-hi",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2 mb-2">
        <span className="hud-label truncate">{label}</span>
        {icon && <span className="text-txt-muted shrink-0">{icon}</span>}
      </div>
      <p className={cx("font-display font-bold text-[1.9rem] leading-none tabular truncate", highlight ? "text-neon" : "text-txt")}>
        {value}
      </p>
      {hint && <p className="text-[11px] text-txt-muted mt-1.5 truncate">{hint}</p>}
    </div>
  );
}
