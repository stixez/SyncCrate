import { forwardRef, type InputHTMLAttributes, type ReactNode } from "react";
import { cx } from "./cx";

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  /** Mono font + tracking — codes, IPs, ports, paths. */
  mono?: boolean;
  size?: "sm" | "md";
  /** Mono micro-label rendered above the field. */
  label?: ReactNode;
  /** Icon inside the field on the left. */
  icon?: ReactNode;
  wrapperClassName?: string;
}

const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { mono, size = "md", label, icon, className, wrapperClassName, id, ...rest },
  ref,
) {
  const field = (
    <div className={cx("relative", !label && wrapperClassName)}>
      {icon && <span className="absolute left-3 top-1/2 -translate-y-1/2 text-txt-muted pointer-events-none">{icon}</span>}
      <input
        ref={ref}
        id={id}
        className={cx("input", mono && "input-mono", size === "sm" && "input-sm", icon ? "pl-9" : undefined, className)}
        {...rest}
      />
    </div>
  );
  if (!label) return field;
  return (
    <label className={cx("block", wrapperClassName)} htmlFor={id}>
      <span className="hud-label block mb-1.5">{label}</span>
      {field}
    </label>
  );
});

export default Input;
