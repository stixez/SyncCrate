import { AlertTriangle, Clock, Sparkles } from "lucide-react";
import type { FileInfo, Resolution } from "../lib/types";
import { formatBytes, formatDate } from "../lib/utils";

interface ConflictResolverProps {
  localFile: FileInfo;
  remoteFile: FileInfo;
  onResolve: (resolution: Resolution) => void;
}

export default function ConflictResolver({ localFile, remoteFile, onResolve }: ConflictResolverProps) {
  const name = localFile.relative_path.split(/[/\\]/).pop() || localFile.relative_path;
  const localNewer = localFile.modified > remoteFile.modified;
  const remoteNewer = remoteFile.modified > localFile.modified;

  return (
    <div className="bg-status-red/5 border border-status-red/30 rounded-xl p-4">
      <div className="flex items-center gap-2 mb-3">
        <AlertTriangle size={16} className="text-status-red" />
        <span className="font-medium text-sm">Conflict: {name}</span>
      </div>
      <div className="grid grid-cols-2 gap-4 mb-3">
        <div className={`bg-bg rounded-lg p-3 ${localNewer ? "ring-1 ring-accent/50" : ""}`}>
          <p className="text-xs font-medium text-accent-light mb-1">
            Your Version {localNewer && <span className="text-[10px] text-accent-light">(newer)</span>}
          </p>
          <p className="text-xs text-txt-dim">Size: {formatBytes(localFile.size)}</p>
          <p className="text-xs text-txt-dim font-mono">Hash: {localFile.hash.slice(0, 12)}</p>
          {localFile.modified > 0 && (
            <p className="text-xs text-txt-dim flex items-center gap-1 mt-1">
              <Clock size={10} /> {formatDate(localFile.modified)}
            </p>
          )}
        </div>
        <div className={`bg-bg rounded-lg p-3 ${remoteNewer ? "ring-1 ring-status-green/50" : ""}`}>
          <p className="text-xs font-medium text-status-green mb-1">
            Their Version {remoteNewer && <span className="text-[10px] text-status-green">(newer)</span>}
          </p>
          <p className="text-xs text-txt-dim">Size: {formatBytes(remoteFile.size)}</p>
          <p className="text-xs text-txt-dim font-mono">Hash: {remoteFile.hash.slice(0, 12)}</p>
          {remoteFile.modified > 0 && (
            <p className="text-xs text-txt-dim flex items-center gap-1 mt-1">
              <Clock size={10} /> {formatDate(remoteFile.modified)}
            </p>
          )}
        </div>
      </div>
      <div className="flex gap-2">
        <button
          onClick={() => onResolve("KeepMine")}
          className="flex-1 bg-accent/20 hover:bg-accent/30 text-accent-light rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
        >
          Keep Mine
        </button>
        <button
          onClick={() => onResolve("UseTheirs")}
          className="flex-1 bg-status-green/20 hover:bg-status-green/30 text-status-green rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
        >
          Use Theirs
        </button>
        <button
          onClick={() => onResolve("KeepBoth")}
          className="flex-1 bg-status-yellow/20 hover:bg-status-yellow/30 text-status-yellow rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
        >
          Keep Both
        </button>
        <button
          onClick={() => onResolve(localNewer ? "KeepMine" : "UseTheirs")}
          className="flex-1 bg-status-green/20 hover:bg-status-green/30 text-status-green rounded-lg px-3 py-1.5 text-xs font-medium transition-colors flex items-center justify-center gap-1"
          title="Automatically keep whichever version was modified more recently"
        >
          <Sparkles size={10} />
          Use Newest (Recommended)
        </button>
      </div>
    </div>
  );
}
