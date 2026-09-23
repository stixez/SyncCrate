import type { ReactNode } from "react";
import { cx } from "./cx";

export interface EmptyStateProps {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  /** Button(s) to get out of the empty state. */
  action?: ReactNode;
  /** Mono caption above the title, defaults to "// Nothing here". */
  label?: ReactNode;
  className?: string;
}

/** Left-aligned empty state inside a dashed hairline frame — no giant centered icon. */
export default function EmptyState({ icon, title, description, action, label = "// Nothing here", className }: EmptyStateProps) {
  return (
    <div className={cx("border border-dashed border-line-hi px-5 py-6 flex items-start gap-4", className)}>
      {icon && <div className="w-10 h-10 shrink-0 grid place-items-center border border-border bg-bg text-txt-muted">{icon}</div>}
      <div className="min-w-0 flex-1">
        {label && <p className="hud-label mb-1">{label}</p>}
        <p className="font-display font-semibold uppercase tracking-[0.04em] text-base text-txt">{title}</p>
        {description && <p className="text-sm text-txt-dim mt-1 max-w-[56ch]">{description}</p>}
        {action && <div className="mt-4 flex flex-wrap gap-2">{action}</div>}
      </div>
    </div>
  );
}
