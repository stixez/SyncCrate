import { useState } from "react";
import { Users, Monitor, X, ChevronDown, ChevronRight, Gamepad2, ArrowUpFromLine } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { formatBytes } from "../lib/utils";
import * as cmd from "../lib/commands";
import { useLogStore } from "../stores/useLogStore";
import { Badge, LiveDot, Panel, ProgressBar, cx } from "./ui";

const PACK_TYPE_LABELS: Record<string, string> = {
  ExpansionPack: "Expansion Packs",
  GamePack: "Game Packs",
  StuffPack: "Stuff Packs",
  Kit: "Kits",
};

export default function PeerList() {
  const session = useAppStore((s) => s.session);
  const gameInfo = useAppStore((s) => s.gameInfo);
  const peerDownloadProgress = useAppStore((s) => s.peerDownloadProgress);
  const addLog = useLogStore((s) => s.addLog);
  const [expandedPeer, setExpandedPeer] = useState<string | null>(null);

  const isHost = session?.session_type === "Host";

  const handleKick = async (peerId: string, peerName: string) => {
    try {
      await cmd.disconnectPeer(peerId);
      addLog(`Kicked peer: ${peerName}`, "info");
    } catch (e) {
      addLog(`Failed to kick peer: ${e}`, "error");
    }
  };

  if (!session || session.peers.length === 0) {
    return (
      <Panel label="// Crew" title="Connected peers" icon={<Users size={15} className="text-txt-muted" />}>
        <p className="font-mono text-[11px] text-txt-muted flex items-center gap-2">
          <LiveDot tone="idle" />
          No peers connected yet
        </p>
      </Panel>
    );
  }

  const localPacks = gameInfo?.installed_packs ?? [];
  const hasLocalPacks = localPacks.length > 0;
  const localPackCodes = new Set(localPacks.map((p) => p.id.code));

  return (
    <Panel
      label="// Crew"
      title="Connected peers"
      icon={<Users size={15} className="text-neon" />}
      actions={<Badge tone="neon" dot>{session.peers.length} online</Badge>}
      bodyClassName="!px-0 !pb-0"
    >
      <div className="border-t border-border">
        {session.peers.map((peer) => {
          const peerPacks = peer.game_info?.installed_packs ?? [];
          const peerPackCount = peerPacks.length;
          const isExpanded = expandedPeer === peer.id;

          const dlProgress = peerDownloadProgress[peer.id];
          const isDownloading = dlProgress && dlProgress.file;
          const dlPercent = isDownloading && dlProgress.file_bytes_total > 0
            ? Math.round((dlProgress.file_bytes_sent / dlProgress.file_bytes_total) * 100)
            : 0;

          return (
            <div key={peer.id} className="px-5 py-3 border-b border-border last:border-b-0">
              <div className="flex items-center gap-3">
                <div className="w-9 h-9 shrink-0 grid place-items-center border border-line-hi bg-bg">
                  <Monitor size={15} className="text-accent-light" />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate flex items-center gap-2">
                    <LiveDot />
                    {peer.name}
                  </p>
                  <p className="font-mono text-[11px] text-txt-muted truncate mt-0.5">
                    {/^[0-9a-f.:]+$/i.test(peer.ip) ? (peer.port > 0 ? `${peer.ip}:${peer.port}` : peer.ip) : `${peer.ip} (via join code)`}
                  </p>
                </div>
                {peer.game_info?.game_version && (
                  <Badge tone="neon" icon={<Gamepad2 size={10} />}>v{peer.game_info.game_version}</Badge>
                )}
                <span className="font-mono text-[11px] text-txt-dim tabular whitespace-nowrap">
                  <span className="text-txt">{peer.mod_count}</span> files
                </span>
                {peerPackCount > 0 && (
                  <button
                    onClick={() => setExpandedPeer(isExpanded ? null : peer.id)}
                    aria-expanded={isExpanded}
                    className="font-mono text-[11px] text-txt-dim hover:text-neon flex items-center gap-0.5 transition-colors whitespace-nowrap"
                  >
                    <span className="text-txt">{peerPackCount}</span>&nbsp;packs
                    {isExpanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
                  </button>
                )}
                {isHost && (
                  <button
                    onClick={() => handleKick(peer.id, peer.name)}
                    title="Kick peer"
                    aria-label={`Kick ${peer.name}`}
                    className="ml-1 w-7 h-7 grid place-items-center border border-transparent text-txt-muted hover:text-status-red hover:border-status-red/50 hover:bg-status-red/10 transition-colors"
                  >
                    <X size={14} />
                  </button>
                )}
              </div>
              {isHost && isDownloading && (
                <div className="mt-2.5 ml-12">
                  <div className="flex items-center gap-2 font-mono text-[11px] text-txt-dim mb-1.5">
                    <ArrowUpFromLine size={11} className="text-neon shrink-0" />
                    <span className="truncate flex-1" title={dlProgress.file!}>
                      Sending: <span className="text-txt">{dlProgress.file!.split(/[/\\]/).pop()}</span>
                    </span>
                    <span className="shrink-0 tabular">
                      {formatBytes(dlProgress.file_bytes_sent)} / {formatBytes(dlProgress.file_bytes_total)}
                    </span>
                    <span className="shrink-0 text-neon tabular">{dlPercent}%</span>
                  </div>
                  <ProgressBar value={dlPercent} />
                  {dlProgress.files_sent > 0 && (
                    <p className="font-mono text-[10px] text-txt-muted mt-1">
                      {dlProgress.files_sent} file(s) sent
                    </p>
                  )}
                </div>
              )}
              {isHost && !isDownloading && dlProgress && dlProgress.files_sent > 0 && (
                <div className="mt-2 ml-12 flex items-center gap-2 font-mono text-[11px] text-txt-dim">
                  <ArrowUpFromLine size={11} className="text-status-green" />
                  <span>{dlProgress.files_sent} file(s) sent</span>
                </div>
              )}
              {isExpanded && peerPackCount > 0 && (
                <div className="mt-3 ml-12 space-y-2.5">
                  {!hasLocalPacks && (
                    <p className="font-mono text-[10.5px] text-txt-muted">
                      Detect your packs on the Dashboard to compare
                    </p>
                  )}
                  {Object.entries(
                    peerPacks.reduce<Record<string, typeof peerPacks>>((acc, p) => {
                      const key = p.id.pack_type;
                      if (!acc[key]) acc[key] = [];
                      acc[key].push(p);
                      return acc;
                    }, {})
                  ).map(([type, packs]) => (
                    <div key={type}>
                      <p className="hud-label mb-1">{PACK_TYPE_LABELS[type] ?? type}</p>
                      <div className="flex flex-wrap gap-1">
                        {packs.map((p) => {
                          const youHaveIt = !hasLocalPacks || localPackCodes.has(p.id.code);
                          return (
                            <span
                              key={p.id.code}
                              className={cx(
                                "inline-block px-2 py-0.5 text-[11px] border",
                                youHaveIt
                                  ? "bg-bg border-border text-txt-dim"
                                  : "bg-amber/10 border-amber/40 text-amber",
                              )}
                              title={
                                !hasLocalPacks
                                  ? "Detect your packs to compare"
                                  : youHaveIt
                                    ? "You have this pack"
                                    : "You don't have this pack"
                              }
                            >
                              {p.name}
                            </span>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </Panel>
  );
}
