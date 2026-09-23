import type { ReactNode } from "react";
import { cx } from "./cx";

export interface SectionHeaderProps {
  /** Big display heading. Wrap a word in <span className="text-neon"> for emphasis. */
  title: ReactNode;
  /** Mono caption above, e.g. "// 01 Session". A `<b>` inside is neon. */
  label?: ReactNode;
  /** One-line description under the title. */
  description?: ReactNode;
  /** Right-aligned actions. */
  actions?: ReactNode;
  size?: "md" | "lg";
  className?: string;
}

export default function SectionHeader({ title, label, description, actions, size = "md", className }: SectionHeaderProps) {
  return (
    <div className={cx("flex items-end justify-between gap-4 flex-wrap", className)}>
      <div className="min-w-0">
        {label && <p className="hud-label mb-2">{label}</p>}
        <h2 className={cx("display text-txt", size === "lg" ? "text-[2.4rem]" : "text-[1.6rem]")}>{title}</h2>
        {description && <p className="text-sm text-txt-dim mt-2 max-w-[60ch]">{description}</p>}
      </div>
      {actions && <div className="flex items-center gap-2 flex-wrap">{actions}</div>}
    </div>
  );
}
