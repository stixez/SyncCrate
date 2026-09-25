import { friendlyError } from "./errors";
import { toast } from "sonner";

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
export function toastAction(message: string, actionLabel: string, onAction: () => void) {
  toast(message, { action: { label: actionLabel, onClick: onAction } });
}
