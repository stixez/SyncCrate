import { useEffect, useMemo, useState } from "react";
import { Sparkles, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import { metaLookup } from "../lib/modMeta";
import { fileName, formatRelative } from "../lib/utils";
import * as cmd from "../lib/commands";
import type { ModMeta } from "../lib/types";
import { Badge, Panel } from "./ui";

const SHOWN = 8;

/**
 * What the last sync brought ("Alex added 12 mods: …") instead of only a
 * file count. Files are grouped the way people think of mods: a creator's
 * folder is one mod, a loose file is one mod, named from the mod's own info
 * where it has any.
 */
export default function WhatsNew({ gameId }: { gameId: string }) {
  const changes = useAppStore((s) => s.lastSyncChanges);
  const [metas, setMetas] = useState<ModMeta[]>([]);
  const mine = changes && changes.game === gameId ? changes : null;

  useEffect(() => {
    if (!mine) return;
    let cancelled = false;
    cmd.getModMetadata(gameId).then((m) => { if (!cancelled) setMetas(m); }).catch(() => {});
    return () => { cancelled = true; };
  }, [mine, gameId]);

  const groups = useMemo(() => {
    if (!mine) return null;
    const folders = (getGameDef(gameId)?.content_types ?? []).map((c) => c.folder.replace(/\/$/, "") + "/").filter((f) => f !== "./");
    const metaFor = metaLookup(metas);
    const group = (paths: string[]) => {
      const names = new Map<string, number>();
      for (const p of paths) {
        const folder = folders.find((f) => p.toLowerCase().startsWith(f.toLowerCase()));
        const inner = folder ? p.slice(folder.length) : p;
        const slash = inner.indexOf("/");
        const unit = slash >= 0 ? inner.slice(0, slash) : fileName(inner).replace(/\.disabled$/i, "");
        const name = metaFor(p)?.name || unit;
        names.set(name, (names.get(name) ?? 0) + 1);
      }
      return [...names.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    };
    return { added: group(mine.added), updated: group(mine.updated), removed: group(mine.removed) };
  }, [mine, metas, gameId]);

  if (!mine || !groups) return null;

  const row = (label: string, tone: "neon" | "amber" | "red", count: number, entries: [string, number][]) =>
    count > 0 && (
      <div className="flex items-start gap-3">
        <Badge tone={tone} className="shrink-0 mt-0.5">{label} {count}</Badge>
        <p className="text-[12.5px] text-txt-dim leading-relaxed min-w-0">
          {entries.slice(0, SHOWN).map(([name, n], i) => (
            <span key={name}>
              {i > 0 && ", "}
              <span className="text-txt">{name}</span>
              {n > 1 && <span className="text-txt-muted"> ({n} files)</span>}
            </span>
          ))}
          {entries.length > SHOWN && <span className="text-txt-muted">, and {entries.length - SHOWN} more</span>}
        </p>
      </div>
    );

  return (
    <Panel
      label={<><b>// What's new</b> &nbsp;{formatRelative(Math.floor(mine.at / 1000))}</>}
      title={mine.from ? `From ${mine.from}` : "Last sync"}
      icon={<Sparkles size={15} className="text-neon" />}
      actions={
        <button onClick={() => useAppStore.getState().setLastSyncChanges(null)} className="p-1 text-txt-muted hover:text-txt" aria-label="Dismiss">
          <X size={15} />
        </button>
      }
      bodyClassName="space-y-2"
    >
      {row("New", "neon", mine.added_count, groups.added)}
      {row("Updated", "amber", mine.updated_count, groups.updated)}
      {row("Removed", "red", mine.removed_count, groups.removed)}
    </Panel>
  );
}
