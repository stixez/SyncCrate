import { useState } from "react";
import { Users, Monitor, X, ChevronDown, ChevronRight, Gamepad2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import * as cmd from "../lib/commands";
import { useLogStore } from "../stores/useLogStore";

const PACK_TYPE_LABELS: Record<string, string> = {
  ExpansionPack: "Expansion Packs",
  GamePack: "Game Packs",
  StuffPack: "Stuff Packs",
  Kit: "Kits",
};

export default function PeerList() {
  const session = useAppStore((s) => s.session);
  const gameInfo = useAppStore((s) => s.gameInfo);
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
      <div className="bg-bg-card rounded-xl border border-border p-4">
        <div className="flex items-center gap-2 mb-3">
          <Users size={16} className="text-txt-dim" />
          <h3 className="font-semibold text-sm">Connected Peers</h3>
        </div>
        <p className="text-xs text-txt-dim">No peers connected yet</p>
      </div>
    );
  }

  const localPacks = gameInfo?.installed_packs ?? [];
  const hasLocalPacks = localPacks.length > 0;
  const localPackCodes = new Set(localPacks.map((p) => p.id.code));

  return (
    <div className="bg-bg-card rounded-xl border border-border p-4">
      <div className="flex items-center gap-2 mb-3">
        <Users size={16} className="text-status-green" />
        <h3 className="font-semibold text-sm">Connected Peers</h3>
      </div>
      <div className="space-y-2">
        {session.peers.map((peer) => {
          const peerPacks = peer.game_info?.installed_packs ?? [];
          const peerPackCount = peerPacks.length;
          const isExpanded = expandedPeer === peer.id;

          return (
            <div key={peer.id} className="bg-bg rounded-lg px-3 py-2">
              <div className="flex items-center gap-3">
                <Monitor size={14} className="text-accent-light" />
                <div className="flex-1">
                  <p className="text-sm font-medium">{peer.name}</p>
                  <p className="text-xs text-txt-dim">{peer.ip}:{peer.port}</p>
                </div>
                {peer.game_info?.game_version && (
                  <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-accent/15 text-accent-light text-xs font-medium">
                    <Gamepad2 size={10} />
                    v{peer.game_info.game_version}
                  </span>
                )}
                <span className="text-xs text-txt-dim">{peer.mod_count} mods</span>
                {peerPackCount > 0 && (
                  <button
                    onClick={() => setExpandedPeer(isExpanded ? null : peer.id)}
                    className="text-xs text-txt-dim hover:text-txt flex items-center gap-0.5 transition-colors"
                  >
                    {peerPackCount} packs
                    {isExpanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
                  </button>
                )}
                <span className="w-2 h-2 rounded-full bg-status-green" />
                {isHost && (
                  <button
                    onClick={() => handleKick(peer.id, peer.name)}
                    title="Kick peer"
                    className="ml-1 p-1 rounded hover:bg-status-red/20 text-txt-dim hover:text-status-red transition-colors"
                  >
                    <X size={14} />
                  </button>
                )}
              </div>
              {isExpanded && peerPackCount > 0 && (
                <div className="mt-2 ml-7 space-y-1.5">
                  {!hasLocalPacks && (
                    <p className="text-[10px] text-txt-dim italic mb-1">
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
                      <p className="text-xs font-medium text-txt-dim mb-0.5">{PACK_TYPE_LABELS[type] ?? type}</p>
                      <div className="flex flex-wrap gap-1">
                        {packs.map((p) => {
                          const youHaveIt = !hasLocalPacks || localPackCodes.has(p.id.code);
                          return (
                            <span
                              key={p.id.code}
                              className={`inline-block px-2 py-0.5 rounded text-xs border ${
                                youHaveIt
                                  ? "bg-bg border-border text-txt-dim"
                                  : "bg-status-yellow/10 border-status-yellow/30 text-status-yellow"
                              }`}
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
    </div>
  );
}
