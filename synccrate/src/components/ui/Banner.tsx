import type { ReactNode } from "react";
import { cx } from "./cx";

export type BannerTone = "info" | "warn" | "danger" | "success";

export interface BannerProps {
  tone?: BannerTone;
  icon?: ReactNode;
  /** Bold first line. */
  title?: ReactNode;
  children?: ReactNode;
  /** Buttons on the right. */
  actions?: ReactNode;
  className?: string;
}

const LINE: Record<BannerTone, string> = {
  info: "border-l-neon bg-neon/[0.06]",
  warn: "border-l-amber bg-amber/[0.07]",
  danger: "border-l-status-red bg-status-red/[0.07]",
  success: "border-l-status-green bg-status-green/[0.06]",
};
const ICON: Record<BannerTone, string> = {
  info: "text-neon",
  warn: "text-amber",
  danger: "text-status-red",
  success: "text-status-green",
};

/** Inline notice: 2px colored left rule on a faint tint (the website's TIP row). */
export default function Banner({ tone = "info", icon, title, children, actions, className }: BannerProps) {
  return (
    <div className={cx("border-l-2 px-4 py-3 flex items-center gap-3", LINE[tone], className)} role={tone === "danger" ? "alert" : undefined}>
      {icon && <span className={cx("shrink-0 self-start mt-0.5", ICON[tone])}>{icon}</span>}
      <div className="flex-1 min-w-0 text-xs text-txt-dim leading-relaxed">
        {title && <p className="text-[13px] font-medium text-txt leading-snug">{title}</p>}
        {children && <div className={title ? "mt-0.5" : undefined}>{children}</div>}
      </div>
      {actions && <div className="shrink-0 flex items-center gap-2">{actions}</div>}
    </div>
  );
}
