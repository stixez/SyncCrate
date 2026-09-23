import type { ReactNode } from "react";
import { cx } from "./cx";

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Visible label; clicking it toggles too. */
  label?: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
  /** "switch" (default) = sliding switch; "check" = square checkbox for dense lists. */
  kind?: "switch" | "check";
  className?: string;
}

export default function Toggle({ checked, onChange, label, description, disabled, kind = "switch", className }: ToggleProps) {
  return (
    <label className={cx("flex items-start gap-2.5 select-none", disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer", className)}>
      {kind === "check" ? (
        <input
          type="checkbox"
          className="check mt-[3px]"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
        />
      ) : (
        <span className="relative inline-block w-8 h-[18px] mt-[1px] shrink-0">
          <input
            type="checkbox"
            role="switch"
            className="peer sr-only"
            checked={checked}
            disabled={disabled}
            onChange={(e) => onChange(e.target.checked)}
          />
          <span
            className={cx(
              "block w-8 h-[18px] border transition-colors peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-neon peer-focus-visible:outline-offset-2",
              checked ? "bg-neon/15 border-neon" : "bg-bg border-line-hi",
            )}
          />
          <span
            className={cx(
              "absolute top-[4px] left-[4px] w-[10px] h-[10px] transition-transform",
              checked ? "translate-x-[14px] bg-neon" : "bg-txt-muted",
            )}
          />
        </span>
      )}
      {(label || description) && (
        <span className="min-w-0">
          {label && <span className="block text-sm text-txt leading-5">{label}</span>}
          {description && <span className="block text-xs text-txt-dim mt-0.5">{description}</span>}
        </span>
      )}
    </label>
  );
}
