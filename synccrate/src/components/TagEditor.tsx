import { useState, useEffect, useRef } from "react";
import { toastError } from "../lib/toast";
import { friendlyError } from "../lib/errors";
import { X, Plus, Tag } from "lucide-react";
import * as cmd from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";
import { Button, cx } from "./ui";

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

  // Shown right away; put back if saving fails, or the chips show tags that
  // were never saved. Only the latest save may roll back: an older failure
  // must not undo a newer save that went through.
  const saveSeq = useRef(0);
  const saveTags = async (next: string[]) => {
    const prev = tags;
    const seq = ++saveSeq.current;
    setTags(next);
    try {
      await cmd.setModTags(useAppStore.getState().selectedGame ?? "", filePath, next);
      onTagsChanged(filePath, next);
    } catch (e) {
      if (seq === saveSeq.current) setTags(prev);
      toastError(`Couldn't save the tags: ${friendlyError(e)}`);
    }
  };

  const toggleTag = (tag: string) => saveTags(tags.includes(tag) ? tags.filter((t) => t !== tag) : [...tags, tag]);

  const addCustom = async () => {
    const trimmed = customInput.trim().slice(0, 32);
    if (!trimmed || tags.includes(trimmed)) {
      setCustomInput("");
      return;
    }
    setCustomInput("");
    await saveTags([...tags, trimmed]);
  };

  return (
    <div
      ref={ref}
      // The editor renders inside a clickable ModItem row; without this every
      // click in the popover also opened the mod details dialog.
      onClick={(e) => e.stopPropagation()}
      className="absolute right-0 top-full mt-1 z-50 box border-line-hi shadow-[0_18px_40px_-12px_rgb(0_0_0/0.6)] p-3 w-72 cursor-default"
    >
      <div className="flex items-center justify-between mb-3">
        <p className="hud-label flex items-center gap-1.5">
          <Tag size={11} />
          <b>//</b> Tags
        </p>
        <button onClick={onClose} className="text-txt-muted hover:text-txt" aria-label="Close tag editor">
          <X size={14} />
        </button>
      </div>

      <div className="flex flex-wrap gap-1.5 mb-3">
        {predefined.map((tag) => (
          <button
            key={tag}
            onClick={() => toggleTag(tag)}
            className={cx(
              "tag h-[22px] px-2 transition-colors",
              tags.includes(tag) ? "text-neon bg-neon/10" : "tag-neutral hover:text-txt hover:border-txt-muted",
            )}
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
              <span key={tag} className="tag h-[22px] px-2 text-accent-light">
                {tag}
                <button onClick={() => toggleTag(tag)} className="hover:text-txt" aria-label={`Remove tag ${tag}`}>
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
          aria-label="Custom tag"
          className="input input-sm input-mono flex-1"
        />
        <Button size="sm" onClick={addCustom} aria-label="Add custom tag" icon={<Plus size={13} />} className="!h-[30px]" />
      </div>
    </div>
  );
}
