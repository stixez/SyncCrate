import type { ReactNode } from "react";
import { cx } from "./cx";

export interface ProgressBarProps {
  /** 0–100. Clamped. */
  value: number;
  /** Mono caption on the left above the bar. */
  label?: ReactNode;
  /** Right caption (defaults to the percentage when a label is given). */
  meta?: ReactNode;
  className?: string;
  /** Name for screen readers (defaults to a text `label`). */
  ariaLabel?: string;
}

export default function ProgressBar({ value, label, meta, className, ariaLabel }: ProgressBarProps) {
  const pct = Math.max(0, Math.min(100, value));
  return (
    <div className={className}>
      {(label || meta !== undefined) && (
        <div className="flex items-center justify-between gap-3 mb-1.5 font-mono text-[11px] text-txt-dim">
          <span className="truncate">{label}</span>
          <span className="tabular shrink-0">{meta ?? `${Math.round(pct)}%`}</span>
        </div>
      )}
      <div className={cx("progress")} role="progressbar" aria-label={ariaLabel ?? (typeof label === "string" ? label : undefined)} aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
        <i style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
