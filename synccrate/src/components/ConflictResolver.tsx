import { AlertTriangle, Clock, Sparkles } from "lucide-react";
import type { FileInfo, Resolution } from "../lib/types";
import { formatBytes, formatDate } from "../lib/utils";
import { Button, cx } from "./ui";

interface ConflictResolverProps {
  localFile: FileInfo;
  remoteFile: FileInfo;
  onResolve: (resolution: Resolution) => void;
}

function VersionCard({ who, file, newer }: { who: string; file: FileInfo; newer: boolean }) {
  return (
    <div className={cx("relative bg-bg border px-3.5 py-3", newer ? "border-neon/50" : "border-border")}>
      {newer && <span className="absolute left-[-1px] top-[-1px] bottom-[-1px] w-[2px] bg-neon" />}
      <div className="flex items-center justify-between gap-2 mb-2">
        <p className="hud-label">{who}</p>
        {newer && <span className="tag text-neon">Newer</span>}
      </div>
      <p className="font-display font-bold text-xl leading-none tabular">{formatBytes(file.size)}</p>
      <p className="font-mono text-[10.5px] text-txt-muted mt-2">#{file.hash.slice(0, 12)}</p>
      {file.modified > 0 && (
        <p className="font-mono text-[10.5px] text-txt-dim flex items-center gap-1.5 mt-1">
          <Clock size={10} /> {formatDate(file.modified)}
        </p>
      )}
    </div>
  );
}

export default function ConflictResolver({ localFile, remoteFile, onResolve }: ConflictResolverProps) {
  const name = localFile.relative_path.split(/[/\\]/).pop() || localFile.relative_path;
  const localNewer = localFile.modified > remoteFile.modified;
  const remoteNewer = remoteFile.modified > localFile.modified;

  return (
    <div className="panel panel-warn">
      <div className="p-4">
        <div className="flex items-center gap-2.5 mb-3 min-w-0">
          <AlertTriangle size={15} className="text-amber shrink-0" />
          <span className="hud-label text-amber shrink-0">Conflict</span>
          <span className="text-sm font-medium truncate" title={localFile.relative_path}>{name}</span>
        </div>
        <div className="grid grid-cols-2 gap-3 mb-3">
          <VersionCard who="// Your version" file={localFile} newer={localNewer} />
          <VersionCard who="// Their version" file={remoteFile} newer={remoteNewer} />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            size="sm"
            icon={<Sparkles size={12} />}
            onClick={() => onResolve(localNewer ? "KeepMine" : "UseTheirs")}
            title="Automatically keep whichever version was modified more recently"
          >
            Use newest
          </Button>
          <Button size="sm" onClick={() => onResolve("KeepMine")}>Keep mine</Button>
          <Button size="sm" onClick={() => onResolve("UseTheirs")}>Use theirs</Button>
          <Button size="sm" variant="ghost" onClick={() => onResolve("KeepBoth")}>Keep both</Button>
        </div>
      </div>
    </div>
  );
}
