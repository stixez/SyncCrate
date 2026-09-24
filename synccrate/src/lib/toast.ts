import { toast } from "sonner";

export function toastSuccess(message: string) {
  toast.success(message);
}

export function toastError(message: string) {
  toast.error(message);
}

export function toastInfo(message: string) {
  toast(message);
}

/** A toast with a single action button (e.g. "Undo"). */
export function toastAction(message: string, actionLabel: string, onAction: () => void) {
  toast(message, { action: { label: actionLabel, onClick: onAction } });
}
