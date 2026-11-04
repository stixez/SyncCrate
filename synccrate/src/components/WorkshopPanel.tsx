import { useEffect, useMemo, useState } from "react";
import { Check, ExternalLink, Info, RefreshCw } from "lucide-react";
import type { WorkshopItem } from "../lib/types";
import * as cmd from "../lib/commands";
import { openCreatorLink } from "../lib/links";
import { useAppStore } from "../stores/useAppStore";
import { isDemoMode } from "../lib/demoData";
import { plural } from "../lib/utils";
import { Badge, Banner, Button, Panel } from "./ui";

const SHOWN = 50;

const DEMO_HOST: WorkshopItem[] = [
  { id: "2009463077", name: "Harmony" },
  { id: "818773962", name: "HugsLib" },
  { id: "1541460369", name: "Hospitality" },
  { id: "2023507013", name: null },
];

/** Opens the item in the Steam app (Subscribe works there without a browser
 * login), or its web page when Steam can't be opened. */
function openItem(id: string) {
  cmd.openWorkshopItem(id).catch(() => openCreatorLink(`https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`));
}

/**
 * Steam Workshop mods live in Steam's own folder, so they never sync. A
 * friend sees which of the host's Workshop mods they're missing, each one
 * opening in Steam to subscribe; otherwise this PC's count and why.
 */
export default function WorkshopPanel({ gameId, gameLabel, syncedFolders, hasWorkshop }: {
  gameId: string;
  gameLabel: string;
  syncedFolders: string;
  hasWorkshop: boolean;
}) {
  const demo = isDemoMode() && new URLSearchParams(window.location.search).has("workshop");
  const [mine, setMine] = useState<WorkshopItem[] | null>(null);
  const [showAll, setShowAll] = useState(false);
  // Steam downloads a subscription in the background: look again on request.
  const [checks, setChecks] = useState(0);
  const session = useAppStore((s) => s.session);
  const host = session?.session_type === "Client" ? session.peers[0] : undefined;
  const hostItems = demo ? DEMO_HOST : host && (!host.game_id || host.game_id === gameId) ? host.game_info?.workshop ?? [] : [];
  const hostName = demo ? "Alex" : host?.name ?? "The host";

  useEffect(() => {
    let cancelled = false;
    if (demo) { setMine(DEMO_HOST.slice(0, 1)); return; }
    if (!hasWorkshop) { setMine([]); return; }
    cmd.getWorkshopItems(gameId).then((m) => { if (!cancelled) setMine(m); }).catch(() => { if (!cancelled) setMine([]); });
    return () => { cancelled = true; };
  }, [gameId, hasWorkshop, demo, checks]);

  const missing = useMemo(() => {
    const have = new Set((mine ?? []).map((w) => w.id));
    return hostItems.filter((w) => !have.has(w.id));
  }, [mine, hostItems]);

  if (mine === null) return null;

  if (hostItems.length > 0 && missing.length === 0) {
    return (
      <Banner tone="success" icon={<Check size={14} />} title={`You have all ${plural(hostItems.length, "Workshop mod")} ${hostName} uses`}>
        Workshop mods stay in Steam's folder and never sync; yours already match.
      </Banner>
    );
  }

  if (missing.length > 0) {
    const shown = showAll ? missing : missing.slice(0, SHOWN);
    return (
      <Panel
        label={<b>// Steam Workshop</b>}
        title={`Subscribe to ${hostName}'s Workshop mods (${missing.length})`}
        icon={<ExternalLink size={15} className="text-neon" />}
        actions={<Button size="sm" variant="ghost" onClick={() => setChecks((n) => n + 1)} icon={<RefreshCw size={12} />}>Check again</Button>}
        bodyClassName="space-y-2"
      >
        <p className="text-xs text-txt-dim">
          Workshop mods stay in Steam's folder, so they can't sync. Click <b>Open in Steam</b>, then <b>Subscribe</b>, and Steam downloads each one.
        </p>
        <ul className="border border-border bg-bg divide-y divide-border/60 max-h-72 overflow-y-auto">
          {shown.map((w) => (
            <li key={w.id} className="flex items-center gap-3 px-3 py-2">
              <div className="min-w-0 flex-1">
                <p className="text-[13px] text-txt truncate">{w.name || `Workshop item ${w.id}`}</p>
                <p className="font-mono text-[10.5px] text-txt-muted truncate">{w.id}</p>
              </div>
              <Badge tone="neutral" className="shrink-0">Not subscribed</Badge>
              <Button size="sm" className="shrink-0" onClick={() => openItem(w.id)} icon={<ExternalLink size={12} />}>
                Open in Steam
              </Button>
            </li>
          ))}
        </ul>
        {missing.length > shown.length && (
          <button onClick={() => setShowAll(true)} className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-dim hover:text-neon transition-colors">
            Show all ({missing.length - shown.length} more)
          </button>
        )}
      </Panel>
    );
  }

  if (mine.length === 0) return null;
  return (
    <Banner tone="info" icon={<Info size={14} />} title={`${mine.length} of your ${gameLabel} mods come from the Steam Workshop`}>
      Steam keeps Workshop mods in its own folder, outside the game folder, so SyncCrate can't sync or back them up.
      When you host, friends see your Workshop mods here with a link to subscribe to each one.
      {syncedFolders && <> SyncCrate still syncs {syncedFolders}.</>}
    </Banner>
  );
}
