import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cx } from "./cx";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Icon element rendered before the label (e.g. <RefreshCw size={14} />). */
  icon?: ReactNode;
  /** Stretch to the container width. */
  block?: boolean;
}

// Literal class names so Tailwind's content scan keeps these component classes.
const VARIANT: Record<ButtonVariant, string> = {
  primary: "btn-primary",
  secondary: "btn-secondary",
  ghost: "btn-ghost",
  danger: "btn-danger",
};
const SIZE: Record<ButtonSize, string> = { sm: "btn-sm", md: "", lg: "btn-lg" };

const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon, block, className, children, type = "button", ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={cx("btn", VARIANT[variant], SIZE[size], block && "w-full", className)}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
});

export default Button;
