import { useState, useEffect, useCallback } from "react";
import { Monitor, Users, Package, Save, HardDrive, RefreshCw, AlertTriangle, Lock, Copy, Check, LayoutGrid, Camera, FolderSync } from "lucide-react";
import type { SyncFolderPermissions } from "../lib/types";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { useSession } from "../hooks/useSession";
import { useSync } from "../hooks/useSync";
import { formatBytes } from "../lib/utils";
import * as cmd from "../lib/commands";
import SyncBanner from "./SyncBanner";
import PeerList from "./PeerList";
import ConnectionGuide from "./ConnectionGuide";

export default function Dashboard() {
  const session = useAppStore((s) => s.session);
  const manifest = useAppStore((s) => s.manifest);
  const setManifest = useAppStore((s) => s.setManifest);
  const syncPlan = useAppStore((s) => s.syncPlan);
  const isScanning = useAppStore((s) => s.isScanning);
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const discoveredPeers = useAppStore((s) => s.discoveredPeers);
  const addLog = useLogStore((s) => s.addLog);
  const activeGame = useAppStore((s) => s.activeGame);
  const setActiveGame = useAppStore((s) => s.setActiveGame);
  const { host, join, connectTo, leave, isLoading } = useSession();
  const { computePlan, executeSync, resolveAll, isLoading: isSyncLoading } = useSync();
  const gameLabels: Record<string, string> = { Sims2: "Sims 2", Sims3: "Sims 3", Sims4: "Sims 4" };
  const activeGameLabel = gameLabels[activeGame] || "Sims 4";
  const games = ["Sims2", "Sims3", "Sims4"] as const;

  const handleGameSwitch = async (game: typeof games[number]) => {
    try {
      await cmd.setActiveGame(game);
      setActiveGame(game);
      // Re-scan for the new game
      setIsScanning(true);
      try {
        const m = await cmd.scanFiles(game);
        setManifest(m);
      } catch (scanErr) {
        addLog(`Scan for ${gameLabels[game]}: ${scanErr}`, "warning");
      } finally {
        setIsScanning(false);
      }
    } catch (e) {
      addLog(`Failed to switch game: ${e}`, "error");
    }
  };
  const [hostName, setHostName] = useState("");
  const [usePin, setUsePin] = useState(false);
  const [folderPerms, setFolderPerms] = useState<SyncFolderPermissions>({
    mods: true,
    saves: true,
    tray: true,
    screenshots: true,
  });
  const [pinCopied, setPinCopied] = useState(false);
  const [pinInput, setPinInput] = useState("");
  const [pinPeerId, setPinPeerId] = useState<string | null>(null);

  const [localVersion, setLocalVersion] = useState("");
  useEffect(() => {
    cmd.getAppVersion().then(setLocalVersion).catch(() => {});
  }, []);

  const isConnected = session && session.session_type !== "None";
  const mismatchedPeers = isConnected && localVersion
    ? session.peers.filter((p) => p.version && p.version !== localVersion)
    : [];

  const modCount = manifest
    ? Object.values(manifest.files).filter((f) => f.file_type === "Mod").length
    : 0;
  const ccCount = manifest
    ? Object.values(manifest.files).filter((f) => f.file_type === "CustomContent").length
    : 0;
  const saveCount = manifest
    ? Object.values(manifest.files).filter((f) => f.file_type === "Save").length
    : 0;
  const trayCount = manifest
    ? Object.values(manifest.files).filter((f) => f.file_type === "Tray").length
    : 0;
  const screenshotCount = manifest
    ? Object.values(manifest.files).filter((f) => f.file_type === "Screenshot").length
    : 0;
  const totalSize = manifest
    ? Object.values(manifest.files).reduce((sum, f) => sum + f.size, 0)
    : 0;

  const handleScan = useCallback(async () => {
    setIsScanning(true);
    try {
      const m = await cmd.scanFiles();
      setManifest(m);
    } catch (e) {
      addLog(`Scan failed: ${e}`, "error");
    } finally {
      setIsScanning(false);
    }
  }, [setIsScanning, setManifest, addLog]);

  useEffect(() => {
    if (!manifest) {
      handleScan();
    }
  }, [manifest, handleScan]);

  if (!isConnected) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <div className="text-center mb-4">
          <h2 className="text-2xl font-bold mb-2">Welcome to SimShare</h2>
          <p className="text-txt-dim">Sync your mods and saves with friends over LAN</p>
        </div>

        <div className="flex justify-center gap-2 mb-4">
          {games.map((g) => (
            <button
              key={g}
              onClick={() => handleGameSwitch(g)}
              className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors border ${
                activeGame === g
                  ? "bg-accent text-white border-accent"
                  : "bg-bg-card border-border text-txt-dim hover:bg-bg-card-hover"
              }`}
            >
              {gameLabels[g]}
            </button>
          ))}
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div className="bg-bg-card rounded-xl border border-border p-6 hover:border-accent/50 transition-colors">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-lg bg-accent/20 flex items-center justify-center">
                <Monitor size={20} className="text-accent-light" />
              </div>
              <h3 className="font-semibold">Host a Session</h3>
            </div>
            <p className="text-txt-dim text-sm mb-4">
              Share your mods and saves with others on your network.
            </p>
            <input
              type="text"
              value={hostName}
              onChange={(e) => setHostName(e.target.value.replace(/[^\w\s-]/g, "").slice(0, 32))}
              maxLength={32}
              placeholder="Your name..."
              className="w-full bg-bg border border-border rounded-lg px-3 py-2 text-sm mb-3 focus:outline-none focus:border-accent"
            />
            <label className="flex items-center gap-2 mb-3 cursor-pointer text-sm text-txt-dim">
              <input
                type="checkbox"
                checked={usePin}
                onChange={(e) => setUsePin(e.target.checked)}
                className="rounded border-border accent-accent"
              />
              <Lock size={14} />
              Require PIN to join
            </label>
            <div className="mb-3">
              <div className="flex items-center gap-1.5 mb-1.5 text-sm text-txt-dim">
                <FolderSync size={14} />
                Shared folders
              </div>
              <div className="grid grid-cols-2 gap-x-3 gap-y-1">
                {([
                  ["mods", "Mods"],
                  ["saves", "Saves"],
                  ["tray", "Tray"],
                  ["screenshots", "Screenshots"],
                ] as const).map(([key, label]) => (
                  <label key={key} className="flex items-center gap-1.5 cursor-pointer text-sm text-txt-dim">
                    <input
                      type="checkbox"
                      checked={folderPerms[key]}
                      onChange={(e) => setFolderPerms((p) => ({ ...p, [key]: e.target.checked }))}
                      className="rounded border-border accent-accent"
                    />
                    {label}
                  </label>
                ))}
              </div>
            </div>
            <button
              onClick={() => host(hostName.trim() || "Host", usePin, folderPerms)}
              disabled={isLoading}
              className="w-full bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50"
            >
              {isLoading ? "Starting..." : "Start Hosting"}
            </button>
          </div>

          <div className="bg-bg-card rounded-xl border border-border p-6 hover:border-accent/50 transition-colors">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-lg bg-status-green/20 flex items-center justify-center">
                <Users size={20} className="text-status-green" />
              </div>
              <h3 className="font-semibold">Join a Session</h3>
            </div>
            <p className="text-txt-dim text-sm mb-4">
              Connect to a host on your network and sync files.
            </p>
            <button
              onClick={() => join(hostName.trim() || "Guest")}
              disabled={isLoading}
              className="w-full bg-status-green/20 hover:bg-status-green/30 text-status-green rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50 mb-3"
            >
              {isLoading ? "Scanning..." : "Scan for Hosts"}
            </button>
            {discoveredPeers.length > 0 && (
              <div className="space-y-2">
                {discoveredPeers.map((peer) => (
                  <button
                    key={peer.id}
                    onClick={() => {
                      if (peer.pin_required) {
                        setPinPeerId(peer.id);
                        setPinInput("");
                      } else {
                        connectTo(peer.id);
                      }
                    }}
                    disabled={isLoading}
                    className="w-full flex items-center justify-between bg-bg rounded-lg px-3 py-2 text-sm hover:bg-bg-card-hover transition-colors disabled:opacity-50"
                  >
                    <span className="flex items-center gap-1.5">
                      {peer.name}
                      {peer.pin_required && <Lock size={12} className="text-txt-dim" />}
                    </span>
                    <span className="text-txt-dim text-xs">{peer.mod_count} mods</span>
                  </button>
                ))}
              </div>
            )}
            {pinPeerId && (
              <div className="mt-3 bg-bg rounded-lg border border-border p-3">
                <p className="text-sm font-medium mb-2">Enter Session PIN</p>
                <div className="flex gap-2">
                  <input
                    type="text"
                    inputMode="numeric"
                    maxLength={4}
                    value={pinInput}
                    onChange={(e) => setPinInput(e.target.value.replace(/\D/g, "").slice(0, 4))}
                    placeholder="0000"
                    className="flex-1 bg-bg-card border border-border rounded-lg px-3 py-2 text-center text-lg font-mono tracking-widest focus:outline-none focus:border-accent"
                    autoFocus
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && pinInput.length === 4) {
                        connectTo(pinPeerId, pinInput);
                        setPinPeerId(null);
                      } else if (e.key === "Escape") {
                        setPinPeerId(null);
                      }
                    }}
                  />
                  <button
                    onClick={() => {
                      connectTo(pinPeerId, pinInput);
                      setPinPeerId(null);
                    }}
                    disabled={pinInput.length !== 4 || isLoading}
                    className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50"
                  >
                    Connect
                  </button>
                </div>
                <button
                  onClick={() => setPinPeerId(null)}
                  className="text-xs text-txt-dim mt-2 hover:text-txt"
                >
                  Cancel
                </button>
              </div>
            )}
          </div>
        </div>

        {manifest && (
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
            {[
              { label: "Script Mods", value: modCount, icon: Package, color: "text-accent-light" },
              { label: "Custom Content", value: ccCount, icon: Package, color: "text-pink-400" },
              { label: "Save Files", value: saveCount, icon: Save, color: "text-status-green" },
              { label: "Tray Items", value: trayCount, icon: LayoutGrid, color: "text-purple-400" },
              { label: "Screenshots", value: screenshotCount, icon: Camera, color: "text-sky-400" },
              { label: "Total Size", value: formatBytes(totalSize), icon: HardDrive, color: "text-status-yellow" },
            ].map(({ label, value, icon: Icon, color }) => (
              <div key={label} className="bg-bg-card rounded-xl border border-border p-4">
                <div className="flex items-center gap-2 mb-2">
                  <Icon size={16} className={color} />
                  <span className="text-txt-dim text-sm">{label}</span>
                </div>
                <p className="text-2xl font-bold">{value}</p>
              </div>
            ))}
          </div>
        )}

        <div className="flex justify-center">
          <button
            onClick={handleScan}
            disabled={isScanning}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors disabled:opacity-50"
          >
            <RefreshCw size={14} className={isScanning ? "animate-spin" : ""} />
            {isScanning ? "Scanning..." : "Scan Files"}
          </button>
        </div>

        <ConnectionGuide />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">Dashboard</h2>
        <div className="flex gap-2">
          <button
            onClick={handleScan}
            disabled={isScanning}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors disabled:opacity-50"
          >
            <RefreshCw size={14} className={isScanning ? "animate-spin" : ""} />
            Scan Files
          </button>
          <button
            onClick={computePlan}
            disabled={isSyncLoading}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-sm transition-colors disabled:opacity-50"
          >
            {isSyncLoading ? "Computing..." : "Compare & Sync"}
          </button>
          <button
            onClick={leave}
            disabled={isLoading}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-status-red/20 hover:bg-status-red/30 text-status-red text-sm transition-colors disabled:opacity-50"
          >
            Disconnect
          </button>
        </div>
      </div>

      {session.session_type === "Host" && session.pin && (
        <div className="bg-accent/10 border border-accent/30 rounded-xl p-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Lock size={18} className="text-accent-light" />
            <div>
              <p className="text-xs text-txt-dim">Session PIN</p>
              <p className="text-2xl font-bold font-mono tracking-[0.3em]">{session.pin}</p>
            </div>
          </div>
          <button
            onClick={() => {
              navigator.clipboard.writeText(session.pin!);
              setPinCopied(true);
              setTimeout(() => setPinCopied(false), 2000);
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors"
          >
            {pinCopied ? <Check size={14} className="text-status-green" /> : <Copy size={14} />}
            {pinCopied ? "Copied" : "Copy"}
          </button>
        </div>
      )}

      {mismatchedPeers.length > 0 && (
        <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-3 flex items-center gap-2">
          <AlertTriangle size={16} className="text-status-yellow shrink-0" />
          <span className="text-sm text-status-yellow">
            Version mismatch: {mismatchedPeers.map((p) => `${p.name} (v${p.version})`).join(", ")} — you have v{localVersion}. Update both to the same version for best results.
          </span>
        </div>
      )}

      {syncPlan && syncPlan.actions.length > 0 && (
        <SyncBanner plan={syncPlan} onSync={executeSync} onResolveAll={resolveAll} />
      )}
      {syncPlan && syncPlan.actions.length === 0 && (
        <div className="bg-status-green/10 border border-status-green/30 rounded-xl p-4 text-center">
          <span className="text-sm text-status-green font-medium">Everything is in sync!</span>
        </div>
      )}

      <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
        {[
          { label: "Script Mods", value: modCount, icon: Package, color: "text-accent-light" },
          { label: "Custom Content", value: ccCount, icon: Package, color: "text-pink-400" },
          { label: "Save Files", value: saveCount, icon: Save, color: "text-status-green" },
          { label: "Tray Items", value: trayCount, icon: LayoutGrid, color: "text-purple-400" },
          { label: "Screenshots", value: screenshotCount, icon: Camera, color: "text-sky-400" },
          { label: "Total Size", value: formatBytes(totalSize), icon: HardDrive, color: "text-status-yellow" },
        ].map(({ label, value, icon: Icon, color }) => (
          <div key={label} className="bg-bg-card rounded-xl border border-border p-4">
            <div className="flex items-center gap-2 mb-2">
              <Icon size={16} className={color} />
              <span className="text-txt-dim text-sm">{label}</span>
            </div>
            <p className="text-2xl font-bold">{value}</p>
          </div>
        ))}
      </div>

      <PeerList />
    </div>
  );
}
