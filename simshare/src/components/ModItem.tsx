import { useState } from "react";
import { Puzzle, Palette, Tag } from "lucide-react";
import type { FileInfo } from "../lib/types";
import { formatBytes } from "../lib/utils";
import StatusBadge from "./StatusBadge";
import TagEditor from "./TagEditor";

interface ModItemProps {
  file: FileInfo;
  syncStatus?: "synced" | "pending" | "conflict" | "local";
  tags?: string[];
  onTagsChanged?: (path: string, tags: string[]) => void;
  selected?: boolean;
  onSelect?: (path: string) => void;
  bulkMode?: boolean;
}

export default function ModItem({
  file,
  syncStatus = "local",
  tags = [],
  onTagsChanged,
  selected,
  onSelect,
  bulkMode,
}: ModItemProps) {
  const isMod = file.file_type === "Mod";
  const name = file.relative_path.split(/[/\\]/).pop() || file.relative_path;
  const [showTagEditor, setShowTagEditor] = useState(false);

  return (
    <div className="relative flex items-center gap-3 bg-bg-card rounded-lg border border-border px-4 py-3 hover:bg-bg-card-hover transition-colors">
      {bulkMode && (
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onSelect?.(file.relative_path)}
          className="shrink-0 accent-accent"
        />
      )}
      <div className={`w-8 h-8 rounded-lg flex items-center justify-center ${isMod ? "bg-accent/20" : "bg-pink-500/20"}`}>
        {isMod ? <Puzzle size={16} className="text-accent-light" /> : <Palette size={16} className="text-pink-400" />}
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate">{name}</p>
        <div className="flex items-center gap-1.5 mt-0.5">
          <p className="text-xs text-txt-dim truncate">{file.relative_path}</p>
          {tags.length > 0 && (
            <div className="flex gap-1 shrink-0">
              {tags.slice(0, 3).map((tag) => (
                <span
                  key={tag}
                  className="px-1.5 py-0 rounded-full bg-accent/15 text-accent-light text-[10px] font-medium"
                >
                  {tag}
                </span>
              ))}
              {tags.length > 3 && (
                <span className="text-[10px] text-txt-dim">+{tags.length - 3}</span>
              )}
            </div>
          )}
        </div>
      </div>
      <span className="text-xs text-txt-dim">{formatBytes(file.size)}</span>
      <span className="text-xs text-txt-dim font-mono">{file.hash.slice(0, 8)}</span>
      <button
        onClick={() => setShowTagEditor(!showTagEditor)}
        className="p-1 rounded hover:bg-bg-card-active transition-colors text-txt-dim hover:text-accent-light"
        title="Edit tags"
      >
        <Tag size={14} />
      </button>
      <StatusBadge status={syncStatus} />
      {showTagEditor && onTagsChanged && (
        <TagEditor
          filePath={file.relative_path}
          currentTags={tags}
          onTagsChanged={onTagsChanged}
          onClose={() => setShowTagEditor(false)}
        />
      )}
    </div>
  );
}
