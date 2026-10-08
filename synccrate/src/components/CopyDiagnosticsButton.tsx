import { useState } from "react";
import { ClipboardCopy, Loader2 } from "lucide-react";
import { copyDiagnostics } from "../lib/diagnostics";
import { Button } from "./ui";
import type { ButtonProps } from "./ui/Button";

/** "Copy diagnostics" for bug reports: builds a scrubbed text report and
 * copies it. Nothing is sent anywhere. */
export default function CopyDiagnosticsButton({ size = "sm", variant = "secondary", className }: Pick<ButtonProps, "size" | "variant" | "className">) {
  const [busy, setBusy] = useState(false);
  return (
    <Button
      size={size}
      variant={variant}
      className={className}
      disabled={busy}
      title="Copies a short report for a bug report. No files, join codes or friends' names."
      icon={busy ? <Loader2 size={12} className="animate-spin" /> : <ClipboardCopy size={12} />}
      onClick={async () => {
        setBusy(true);
        try {
          await copyDiagnostics();
        } finally {
          setBusy(false);
        }
      }}
    >
      Copy diagnostics
    </Button>
  );
}
