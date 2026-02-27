import { CheckCircle, AlertTriangle, XCircle, X, FileWarning } from "lucide-react";
import type { InstallResult } from "../lib/types";

interface InstallResultsModalProps {
  results: InstallResult[];
  onClose: () => void;
  onResolveDuplicate: (source: string, strategy: "overwrite" | "rename") => void;
}

export default function InstallResultsModal({ results, onClose, onResolveDuplicate }: InstallResultsModalProps) {
  const fileName = (path: string) => path.split(/[/\\]/).pop() || path;

  return (
    <div className="fixed inset-0 z-[100] bg-black/50 flex items-center justify-center p-4">
      <div className="bg-bg-card rounded-xl border border-border shadow-xl max-w-lg w-full max-h-[80vh] flex flex-col">
        <div className="flex items-center justify-between p-4 border-b border-border">
          <h3 className="font-semibold">Install Results</h3>
          <button onClick={onClose} className="text-txt-dim hover:text-txt">
            <X size={18} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {results.map((r, i) => (
            <div
              key={i}
              className="flex items-start gap-3 bg-bg rounded-lg border border-border p-3"
            >
              {r.status === "Success" && <CheckCircle size={18} className="text-status-green shrink-0 mt-0.5" />}
              {r.status === "Duplicate" && <FileWarning size={18} className="text-status-yellow shrink-0 mt-0.5" />}
              {r.status === "InvalidExtension" && <AlertTriangle size={18} className="text-status-yellow shrink-0 mt-0.5" />}
              {r.status === "Failed" && <XCircle size={18} className="text-status-red shrink-0 mt-0.5" />}
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium truncate">{fileName(r.source)}</p>
                {r.message && <p className="text-xs text-txt-dim mt-0.5">{r.message}</p>}
                {r.status === "Duplicate" && (
                  <div className="flex gap-2 mt-2">
                    <button
                      onClick={() => onResolveDuplicate(r.source, "overwrite")}
                      className="px-2.5 py-1 rounded bg-status-yellow/20 text-status-yellow text-xs font-medium hover:bg-status-yellow/30 transition-colors"
                    >
                      Overwrite
                    </button>
                    <button
                      onClick={() => onResolveDuplicate(r.source, "rename")}
                      className="px-2.5 py-1 rounded bg-accent/20 text-accent-light text-xs font-medium hover:bg-accent/30 transition-colors"
                    >
                      Rename
                    </button>
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
        <div className="p-4 border-t border-border">
          <button
            onClick={onClose}
            className="w-full bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
