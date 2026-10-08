import { friendlyError } from "./errors";
import { toast } from "sonner";
import { useAppStore } from "../stores/useAppStore";

export function toastSuccess(message: string) {
  toast.success(message);
}

/** Error toasts get plain-language wording for OS/backend errors (see
 * `friendlyError`), including the `${prefix}: ${e}` form most call sites use. */
export function toastError(message: string) {
  const i = message.indexOf(": ");
  const tail = i > 0 && i < 80 ? message.slice(i + 2) : message;
  const friendly = friendlyError(tail);
  toast.error(friendly === tail ? message : i > 0 && i < 80 ? `${message.slice(0, i)}: ${friendly}` : friendly);
}

export function toastInfo(message: string) {
  toast(message);
}

/** A toast with a single action button (e.g. "Undo"). */
export function toastAction(
  message: string,
  actionLabel: string,
  onAction: () => void,
  opts: { duration?: number; tone?: "info" | "error" } = {},
) {
  const show = opts.tone === "error" ? toast.error : toast;
  show(message, { action: { label: actionLabel, onClick: onAction }, duration: opts.duration });
}

/** An error toast (same friendly wording as `toastError`) with one action,
 * e.g. "Copy diagnostics" after a failed connection. */
export function toastErrorAction(message: string, actionLabel: string, onAction: () => void) {
  const i = message.indexOf(": ");
  const tail = i > 0 && i < 80 ? message.slice(i + 2) : message;
  const friendly = friendlyError(tail);
  const text = friendly === tail ? message : i > 0 && i < 80 ? `${message.slice(0, i)}: ${friendly}` : friendly;
  toast.error(text, { action: { label: actionLabel, onClick: onAction }, duration: 10000 });
}

/** For messages that point at the Activity log: a button that opens it. */
export function toastWithLog(message: string, tone: "info" | "error" = "info") {
  toastAction(message, "View log", () => useAppStore.getState().navigateToGlobal("activity"), { tone, duration: 8000 });
}
