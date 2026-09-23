import { CheckCircle, AlertTriangle, XCircle, X, FileWarning } from "lucide-react";
import type { InstallResult } from "../lib/types";
import { Button, cx } from "./ui";

interface InstallResultsModalProps {
  results: InstallResult[];
  onClose: () => void;
  onResolveDuplicate: (source: string, strategy: "overwrite" | "rename") => void;
}

export default function InstallResultsModal({ results, onClose, onResolveDuplicate }: InstallResultsModalProps) {
  const fileName = (path: string) => path.split(/[/\\]/).pop() || path;
  const ok = results.filter((r) => r.status === "Success").length;
  const failed = results.filter((r) => r.status === "Failed").length;
  const attention = results.length - ok - failed;

  return (
    <div className="fixed inset-0 z-[100] bg-black/65 backdrop-blur-[2px] flex items-center justify-center p-4" role="dialog" aria-modal="true" aria-label="Install results">
      <div className="panel panel-accent max-w-lg w-full max-h-[80vh] flex flex-col shadow-2xl">
        <div className="flex items-start justify-between gap-3 px-5 pt-4 pb-3 border-b border-border">
          <div>
            <p className="hud-label mb-1"><b>//</b> Drop install</p>
            <h3 className="display text-[1.35rem]">Install results</h3>
          </div>
          <button onClick={onClose} className="w-8 h-8 grid place-items-center text-txt-dim hover:text-txt hover:bg-bg-card-hover" aria-label="Close">
            <X size={18} />
          </button>
        </div>
        <div className="grid grid-cols-3 border-b border-border bg-bg/60">
          <div className="px-5 py-2.5 border-r border-border">
            <p className="hud-label">Installed</p>
            <p className="font-display font-bold text-xl leading-none mt-1 text-neon tabular">{ok}</p>
          </div>
          <div className="px-5 py-2.5 border-r border-border">
            <p className="hud-label">Needs you</p>
            <p className={cx("font-display font-bold text-xl leading-none mt-1 tabular", attention > 0 ? "text-amber" : "text-txt")}>{attention}</p>
          </div>
          <div className="px-5 py-2.5">
            <p className="hud-label">Failed</p>
            <p className={cx("font-display font-bold text-xl leading-none mt-1 tabular", failed > 0 ? "text-status-red" : "text-txt")}>{failed}</p>
          </div>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-3">
          {results.map((r, i) => (
            <div
              key={i}
              className={cx(
                "flex items-start gap-3 py-2.5 pl-3 border-l-2 border-b border-b-border last:border-b-0",
                r.status === "Success" && "border-l-status-green",
                (r.status === "Duplicate" || r.status === "InvalidExtension") && "border-l-amber",
                r.status === "Failed" && "border-l-status-red",
              )}
            >
              {r.status === "Success" && <CheckCircle size={16} className="text-status-green shrink-0 mt-0.5" />}
              {r.status === "Duplicate" && <FileWarning size={16} className="text-amber shrink-0 mt-0.5" />}
              {r.status === "InvalidExtension" && <AlertTriangle size={16} className="text-amber shrink-0 mt-0.5" />}
              {r.status === "Failed" && <XCircle size={16} className="text-status-red shrink-0 mt-0.5" />}
              <div className="flex-1 min-w-0">
                <p className="font-mono text-[12.5px] text-txt truncate" title={r.source}>{fileName(r.source)}</p>
                {r.message && <p className="text-xs text-txt-dim mt-0.5">{r.message}</p>}
                {r.status === "Duplicate" && (
                  <div className="flex gap-2 mt-2">
                    <Button size="sm" variant="danger" onClick={() => onResolveDuplicate(r.source, "overwrite")}>
                      Overwrite
                    </Button>
                    <Button size="sm" onClick={() => onResolveDuplicate(r.source, "rename")}>
                      Rename
                    </Button>
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
        <div className="px-5 py-4 border-t border-border">
          <Button variant="primary" block onClick={onClose}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}
