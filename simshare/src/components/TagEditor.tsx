import { useState, useEffect, useRef } from "react";
import { X, Plus, Tag } from "lucide-react";
import * as cmd from "../lib/commands";

interface TagEditorProps {
  filePath: string;
  currentTags: string[];
  onTagsChanged: (path: string, tags: string[]) => void;
  onClose: () => void;
}

export default function TagEditor({ filePath, currentTags, onTagsChanged, onClose }: TagEditorProps) {
  const [tags, setTags] = useState<string[]>(currentTags);
  const [predefined, setPredefined] = useState<string[]>([]);
  const [customInput, setCustomInput] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    cmd.getPredefinedTags().then(setPredefined).catch(() => {});
  }, []);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [onClose]);

  const toggleTag = async (tag: string) => {
    const next = tags.includes(tag) ? tags.filter((t) => t !== tag) : [...tags, tag];
    setTags(next);
    try {
      await cmd.setModTags(filePath, next);
      onTagsChanged(filePath, next);
    } catch (e) {
      console.error("Failed to set tags:", e);
    }
  };

  const addCustom = async () => {
    const trimmed = customInput.trim().slice(0, 32);
    if (!trimmed || tags.includes(trimmed)) {
      setCustomInput("");
      return;
    }
    const next = [...tags, trimmed];
    setTags(next);
    setCustomInput("");
    try {
      await cmd.setModTags(filePath, next);
      onTagsChanged(filePath, next);
    } catch (e) {
      console.error("Failed to set tags:", e);
    }
  };

  return (
    <div
      ref={ref}
      className="absolute right-0 top-full mt-1 z-50 bg-bg-card border border-border rounded-xl shadow-lg p-3 w-72"
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-1.5 text-xs font-medium text-txt-dim">
          <Tag size={12} />
          Tags
        </div>
        <button onClick={onClose} className="text-txt-dim hover:text-txt">
          <X size={14} />
        </button>
      </div>

      <div className="flex flex-wrap gap-1.5 mb-3">
        {predefined.map((tag) => (
          <button
            key={tag}
            onClick={() => toggleTag(tag)}
            className={`px-2 py-0.5 rounded-full text-xs font-medium transition-colors ${
              tags.includes(tag)
                ? "bg-accent text-white"
                : "bg-bg border border-border text-txt-dim hover:border-accent/50"
            }`}
          >
            {tag}
          </button>
        ))}
      </div>

      {tags.filter((t) => !predefined.includes(t)).length > 0 && (
        <div className="flex flex-wrap gap-1.5 mb-3">
          {tags
            .filter((t) => !predefined.includes(t))
            .map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-accent/20 text-accent-light text-xs font-medium"
              >
                {tag}
                <button onClick={() => toggleTag(tag)} className="hover:text-white">
                  <X size={10} />
                </button>
              </span>
            ))}
        </div>
      )}

      <div className="flex gap-1.5">
        <input
          type="text"
          value={customInput}
          onChange={(e) => setCustomInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addCustom();
          }}
          maxLength={32}
          placeholder="Custom tag..."
          className="flex-1 bg-bg border border-border rounded-lg px-2 py-1 text-xs focus:outline-none focus:border-accent"
        />
        <button
          onClick={addCustom}
          className="p-1 rounded-lg bg-bg border border-border hover:bg-bg-card-hover transition-colors"
        >
          <Plus size={14} className="text-txt-dim" />
        </button>
      </div>
    </div>
  );
}
