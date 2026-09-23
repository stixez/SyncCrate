import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export type PanelTone = "default" | "accent" | "warn" | "danger" | "sunken";

export interface PanelProps extends Omit<HTMLAttributes<HTMLDivElement>, "title"> {
  /** Display-font heading in the panel header. */
  title?: ReactNode;
  /** Mono `// LABEL` above the title (or alone). */
  label?: ReactNode;
  /** Icon shown before the title. */
  icon?: ReactNode;
  /** Right side of the header (buttons, status). */
  actions?: ReactNode;
  tone?: PanelTone;
  /** Neon corner brackets — reserve for the one hero element on a screen. */
  brackets?: boolean;
  /** Angled corners (default). false = plain hairline box. */
  cut?: boolean;
  /** Body padding; false to control it yourself. */
  padded?: boolean;
  bodyClassName?: string;
}

// Literal class names so Tailwind's content scan keeps these component classes.
const TONE: Record<PanelTone, string> = {
  default: "",
  accent: "panel-accent",
  warn: "panel-warn",
  danger: "panel-danger",
  sunken: "panel-sunken",
};

export default function Panel({
  title,
  label,
  icon,
  actions,
  tone = "default",
  brackets,
  cut = true,
  padded = true,
  className,
  bodyClassName,
  children,
  ...rest
}: PanelProps) {
  const hasHeader = title || label || actions;
  const inner = (
    <div
      className={cx(
        cut ? "panel" : "box",
        cut && TONE[tone],
        !cut && tone === "accent" && "border-neon/45",
        !cut && tone === "warn" && "border-amber/45",
        !cut && tone === "danger" && "border-status-red/45",
        !brackets && className,
      )}
      {...(brackets ? {} : rest)}
    >
      {hasHeader && (
        <div className={cx("flex items-start justify-between gap-3", padded ? "px-5 pt-4" : "", children ? "pb-3" : padded ? "pb-4" : "")}>
          <div className="min-w-0">
            {label && <p className="hud-label mb-1">{label}</p>}
            {title && (
              <h3 className="font-display font-semibold uppercase tracking-[0.06em] text-[0.95rem] leading-tight flex items-center gap-2">
                {icon}
                {title}
              </h3>
            )}
          </div>
          {actions && <div className="flex items-center gap-2 shrink-0">{actions}</div>}
        </div>
      )}
      {children !== undefined && children !== null && children !== false && (
        <div className={cx(padded && (hasHeader ? "px-5 pb-5" : "p-5"), bodyClassName)}>{children}</div>
      )}
    </div>
  );
  if (!brackets) return inner;
  return (
    <div className={cx("corner-brackets", className)} {...rest}>
      {inner}
    </div>
  );
}
