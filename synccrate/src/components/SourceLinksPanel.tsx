import { useEffect, useMemo, useState } from "react";
import { ExternalLink, Link2 } from "lucide-react";
import type { ModMeta, SourceLinkItem } from "../lib/types";
import { metaLookup } from "../lib/modMeta";
import { displayPath, fileName } from "../lib/utils";
import * as cmd from "../lib/commands";
import { openCreatorLink } from "../lib/links";
import { Badge, Button, Panel } from "./ui";

const SHOWN = 50;

/**
 * Friend side of "Share as a link": host files whose creator asks people to
 * download from their own page. They're never in the sync; each row opens
 * the creator's page instead.
 */
export default function SourceLinksPanel({ gameId, items }: { gameId: string; items: SourceLinkItem[] }) {
  const [metas, setMetas] = useState<ModMeta[]>([]);
  const [showAll, setShowAll] = useState(false);
  useEffect(() => {
    let cancelled = false;
    cmd.getModMetadata(gameId).then((m) => { if (!cancelled) setMetas(m); }).catch(() => {});
    return () => { cancelled = true; };
  }, [gameId]);
  // Names only exist for mods this PC already has (a different version).
  const metaFor = useMemo(() => metaLookup(metas), [metas]);
  const shown = showAll ? items : items.slice(0, SHOWN);

  return (
    <Panel
      label={<b>// From the creator</b>}
      title={`Get these from the creator (${items.length})`}
      icon={<Link2 size={15} className="text-neon" />}
      bodyClassName="space-y-2"
    >
      <p className="text-xs text-txt-dim">
        The host shares these as links: the creator asks people to download them from their page. Install them, then Compare again.
      </p>
      <ul className="border border-border bg-bg divide-y divide-border/60 max-h-72 overflow-y-auto">
        {shown.map((item) => {
          const name = metaFor(item.path)?.name || fileName(item.path).replace(/\.disabled$/i, "");
          return (
            <li key={item.path} className="flex items-center gap-3 px-3 py-2">
              <div className="min-w-0 flex-1">
                <p className="text-[13px] text-txt truncate" title={displayPath(item.path)}>
                  {name}
                  {item.label && item.label !== name && <span className="text-txt-muted"> · {item.label}</span>}
                </p>
                <p className="font-mono text-[10.5px] text-txt-muted truncate" title={item.url}>{item.url}</p>
              </div>
              <Badge tone={item.status === "different" ? "amber" : "neutral"} className="shrink-0">
                {item.status === "different" ? "Different version" : "Not installed"}
              </Badge>
              <Button size="sm" className="shrink-0" onClick={() => openCreatorLink(item.url)} icon={<ExternalLink size={12} />}>
                Open link
              </Button>
            </li>
          );
        })}
      </ul>
      {items.length > shown.length && (
        <button onClick={() => setShowAll(true)} className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-dim hover:text-neon transition-colors">
          Show all ({items.length - shown.length} more)
        </button>
      )}
    </Panel>
  );
}
