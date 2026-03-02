import { useState, useEffect, useCallback } from "react";
import { Monitor, Users, Package, Save, HardDrive, RefreshCw, AlertTriangle, Lock, Copy, Check, LayoutGrid, Camera, FolderSync, Gamepad2, ChevronDown, ChevronRight, FolderOpen, Settings } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SyncFolderPermissions, GameInfo } from "../lib/types";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { useSession } from "../hooks/useSession";
import { useSync } from "../hooks/useSync";
import { formatBytes } from "../lib/utils";
import { toastSuccess, toastError } from "../lib/toast";
import * as cmd from "../lib/commands";
import SyncBanner from "./SyncBanner";
import PeerList from "./PeerList";
import ConnectionGuide from "./ConnectionGuide";
import DonationBanner from "./DonationBanner";

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
  const gamePaths = useAppStore((s) => s.gamePaths);
  const setGamePaths = useAppStore((s) => s.setGamePaths);
  const setPage = useAppStore((s) => s.setPage);
  const { host, join, connectTo, leave, isLoading } = useSession();
  const { computePlan, executeSync, resolveAll, isLoading: isSyncLoading, loadingPhase } = useSync();
  const gameLabels: Record<string, string> = { Sims2: "Sims 2", Sims3: "Sims 3", Sims4: "Sims 4" };
  const activeGameLabel = gameLabels[activeGame] || "Sims 4";
  const games = ["Sims2", "Sims3", "Sims4"] as const;

  const handleGameSwitch = async (game: typeof games[number]) => {
    try {
      await cmd.setActiveGame(game);
      setActiveGame(game);
      setPacksExpanded(false);
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
      // Refresh game info for the new game
      try {
        const info = await cmd.getGameInfo(game);
        if (info) setGameInfo(info);
      } catch {
        setGameInfo(null);
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

  const gameInfo = useAppStore((s) => s.gameInfo);
  const setGameInfo = useAppStore((s) => s.setGameInfo);
  const [packsExpanded, setPacksExpanded] = useState(false);
  const [detectingPacks, setDetectingPacks] = useState(false);

  const handleDetectPacks = useCallback(async () => {
    setDetectingPacks(true);
    try {
      const info = await cmd.detectPacks();
      setGameInfo(info);
      const packCount = info?.installed_packs?.length ?? 0;
      toastSuccess(`Detected ${packCount} pack(s)`);
    } catch (e) {
      addLog(`Pack detection failed: ${e}`, "error");
      toastError(`Pack detection failed`);
    } finally {
      setDetectingPacks(false);
    }
  }, [setGameInfo, addLog]);

  // Fetch persisted game paths and active game from backend on mount
  useEffect(() => {
    cmd.getAllGamePaths().then((paths) => {
      const converted: Partial<Record<string, string>> = {};
      for (const [k, v] of Object.entries(paths)) {
        if (v) converted[k] = v;
      }
      setGamePaths(converted);
    }).catch(() => {});
    cmd.getActiveGame().then((game) => {
      setActiveGame(game);
    }).catch(() => {});
  }, [setGamePaths, setActiveGame]);

  useEffect(() => {
    cmd.getGameInfo().then((info) => {
      if (info) setGameInfo(info);
    }).catch(() => {});
  }, [activeGame, setGameInfo]);

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
      const count = Object.keys(m.files).length;
      toastSuccess(`Scan complete — ${count} file(s) found`);
    } catch (e) {
      addLog(`Scan failed: ${e}`, "error");
      toastError(`Scan failed: ${e}`);
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

        {!gamePaths[activeGame] && (
          <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-4 flex items-center gap-4">
            <div className="w-10 h-10 rounded-lg bg-status-yellow/20 flex items-center justify-center shrink-0">
              <FolderOpen size={20} className="text-status-yellow" />
            </div>
            <div className="flex-1">
              <p className="text-sm font-medium">Set your {activeGameLabel} folder</p>
              <p className="text-xs text-txt-dim mt-0.5">SimShare needs to know where your game files are before it can scan.</p>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={async () => {
                  try {
                    const selected = await open({ directory: true });
                    if (selected) {
                      const path = typeof selected === "string" ? selected : selected;
                      await cmd.setGamePath(activeGame, path);
                      setGamePaths({ ...gamePaths, [activeGame]: path });
                      toastSuccess(`${activeGameLabel} path saved`);
                      setIsScanning(true);
                      try {
                        const m = await cmd.scanFiles(activeGame);
                        setManifest(m);
                      } finally {
                        setIsScanning(false);
                      }
                    }
                  } catch (e) {
                    toastError(`Failed to set path`);
                  }
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-status-yellow/20 hover:bg-status-yellow/30 text-status-yellow text-sm font-medium transition-colors"
              >
                <FolderOpen size={14} />
                Browse
              </button>
              <button
                onClick={() => setPage("settings")}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-xs text-txt-dim transition-colors"
              >
                <Settings size={12} />
                Settings
              </button>
            </div>
          </div>
        )}

        <div>
          <label className="text-xs text-txt-dim mb-1.5 block">Your Name</label>
          <input
            type="text"
            value={hostName}
            onChange={(e) => setHostName(e.target.value.replace(/[^\w\s-]/g, "").slice(0, 32))}
            maxLength={32}
            placeholder="Enter your name..."
            aria-label="Your name"
            className="w-full bg-bg-card border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
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
                    <span className="flex items-center gap-2">
                      {peer.game_info?.game_version && (
                        <span className="text-accent-light text-[10px] font-medium bg-accent/15 px-1.5 py-0.5 rounded-full">
                          v{peer.game_info.game_version}
                        </span>
                      )}
                      <span className="text-txt-dim text-xs">{peer.mod_count} files</span>
                    </span>
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
                    aria-label="Session PIN"
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

        <GameInfoCard
          gameInfo={gameInfo}
          activeGameLabel={activeGameLabel}
          packsExpanded={packsExpanded}
          setPacksExpanded={setPacksExpanded}
          detectingPacks={detectingPacks}
          onDetect={handleDetectPacks}
        />

        {isScanning && !manifest ? (
          <div className="space-y-4">
            <div className="flex items-center justify-center gap-3 py-4">
              <RefreshCw size={18} className="animate-spin text-accent-light" />
              <span className="text-sm text-txt-dim">Scanning your {activeGameLabel} files...</span>
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
              {Array.from({ length: 6 }).map((_, i) => (
                <div key={i} className="bg-bg-card rounded-xl border border-border p-4">
                  <div className="flex items-center gap-2 mb-2">
                    <div className="w-4 h-4 rounded animate-pulse bg-bg-card-hover" />
                    <div className="w-20 h-4 rounded animate-pulse bg-bg-card-hover" />
                  </div>
                  <div className="w-12 h-8 rounded animate-pulse bg-bg-card-hover" />
                </div>
              ))}
            </div>
          </div>
        ) : manifest ? (
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
        ) : null}

        <div className="flex justify-center gap-2">
          <button
            onClick={handleScan}
            disabled={isScanning}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors disabled:opacity-50"
          >
            <RefreshCw size={14} className={isScanning ? "animate-spin" : ""} />
            {isScanning ? "Scanning..." : "Scan Files"}
          </button>
          {gamePaths[activeGame] && (
            <button
              onClick={() => cmd.openFolder(gamePaths[activeGame]!)}
              className="flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors"
            >
              <FolderOpen size={14} />
              Open Folder
            </button>
          )}
        </div>

        <ConnectionGuide />
      </div>
    );
  }

  const isHost = session.session_type === "Host";
  const isClient = session.session_type === "Client";
  const hostPeer = isClient && session.peers.length > 0 ? session.peers[0] : null;

  return (
    <div className="space-y-6">
      <DonationBanner />
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
            {isSyncLoading ? (loadingPhase || "Computing...") : "Compare & Sync"}
          </button>
          {gamePaths[activeGame] && (
            <button
              onClick={() => cmd.openFolder(gamePaths[activeGame]!)}
              className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors"
            >
              <FolderOpen size={14} />
              Open Folder
            </button>
          )}
          <button
            onClick={leave}
            disabled={isLoading}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-status-red/20 hover:bg-status-red/30 text-status-red text-sm transition-colors disabled:opacity-50"
          >
            Disconnect
          </button>
        </div>
      </div>

      {isClient && hostPeer && (
        <div className="bg-status-green/10 border border-status-green/30 rounded-xl p-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-status-green/20 flex items-center justify-center">
              <Monitor size={20} className="text-status-green" />
            </div>
            <div className="flex-1">
              <p className="text-xs text-txt-dim">Connected to</p>
              <p className="text-sm font-semibold">{hostPeer.name}</p>
            </div>
            <div className="flex items-center gap-3 text-sm">
              {hostPeer.game_info?.game_version && (
                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-accent/15 text-accent-light text-xs font-medium">
                  <Gamepad2 size={10} />
                  v{hostPeer.game_info.game_version}
                </span>
              )}
              <span className="text-txt-dim text-xs">{hostPeer.mod_count} files</span>
              {(hostPeer.game_info?.installed_packs?.length ?? 0) > 0 && (
                <span className="text-txt-dim text-xs">
                  {hostPeer.game_info!.installed_packs!.length} packs
                </span>
              )}
              <span className="w-2 h-2 rounded-full bg-status-green" />
            </div>
          </div>
        </div>
      )}

      {isHost && session.pin && (
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

      <GameInfoCard
        gameInfo={gameInfo}
        activeGameLabel={activeGameLabel}
        packsExpanded={packsExpanded}
        setPacksExpanded={setPacksExpanded}
        detectingPacks={detectingPacks}
        onDetect={handleDetectPacks}
      />

      {syncPlan && syncPlan.actions.length > 0 && (
        <SyncBanner plan={syncPlan} onSync={executeSync} onResolveAll={resolveAll} />
      )}
      {syncPlan && syncPlan.actions.length === 0 && (
        <div className="bg-status-green/10 border border-status-green/30 rounded-xl p-4 text-center">
          <span className="text-sm text-status-green font-medium">Everything is in sync!</span>
        </div>
      )}

      {isScanning && !manifest ? (
        <div className="space-y-4">
          <div className="flex items-center justify-center gap-3 py-2">
            <RefreshCw size={16} className="animate-spin text-accent-light" />
            <span className="text-sm text-txt-dim">Scanning files...</span>
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="bg-bg-card rounded-xl border border-border p-4">
                <div className="flex items-center gap-2 mb-2">
                  <div className="w-4 h-4 rounded animate-pulse bg-bg-card-hover" />
                  <div className="w-20 h-4 rounded animate-pulse bg-bg-card-hover" />
                </div>
                <div className="w-12 h-8 rounded animate-pulse bg-bg-card-hover" />
              </div>
            ))}
          </div>
        </div>
      ) : (
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

      <PeerList />
    </div>
  );
}

const PACK_TYPE_LABELS: Record<string, string> = {
  ExpansionPack: "Expansion Packs",
  GamePack: "Game Packs",
  StuffPack: "Stuff Packs",
  Kit: "Kits",
};

function GameInfoCard({
  gameInfo,
  activeGameLabel,
  packsExpanded,
  setPacksExpanded,
  detectingPacks,
  onDetect,
}: {
  gameInfo: GameInfo | null;
  activeGameLabel: string;
  packsExpanded: boolean;
  setPacksExpanded: (v: boolean) => void;
  detectingPacks: boolean;
  onDetect: () => void;
}) {
  const packCount = gameInfo?.installed_packs?.length ?? 0;
  const hasVersion = !!gameInfo?.game_version;
  const hasAnyData = hasVersion || packCount > 0;

  const grouped = (gameInfo?.installed_packs ?? []).reduce<Record<string, string[]>>((acc, p) => {
    const key = p.id.pack_type;
    if (!acc[key]) acc[key] = [];
    acc[key].push(p.name);
    return acc;
  }, {});

  return (
    <div className="bg-bg-card rounded-xl border border-border p-4">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <Gamepad2 size={16} className="text-accent-light" />
          <h3 className="font-semibold text-sm">{activeGameLabel} Info</h3>
        </div>
        <button
          onClick={onDetect}
          disabled={detectingPacks}
          className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-xs transition-colors disabled:opacity-50"
        >
          <RefreshCw size={12} className={detectingPacks ? "animate-spin" : ""} />
          {detectingPacks ? "Detecting..." : "Detect Packs"}
        </button>
      </div>
      {hasAnyData ? (
        <>
          <div className="flex items-center gap-4 text-sm">
            {hasVersion && (
              <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-accent/15 text-accent-light text-xs font-medium">
                v{gameInfo!.game_version}
              </span>
            )}
            <span className="text-txt-dim">
              {packCount} {packCount === 1 ? "pack" : "packs"} detected
            </span>
          </div>
          {packCount > 0 && (
            <div className="mt-2">
              <button
                onClick={() => setPacksExpanded(!packsExpanded)}
                className="flex items-center gap-1 text-xs text-txt-dim hover:text-txt transition-colors"
              >
                {packsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                {packsExpanded ? "Hide packs" : "Show installed packs"}
              </button>
              {packsExpanded && (
                <div className="mt-2 space-y-2">
                  {Object.entries(grouped).map(([type, names]) => (
                    <div key={type}>
                      <p className="text-xs font-medium text-txt-dim mb-1">{PACK_TYPE_LABELS[type] ?? type}</p>
                      <div className="flex flex-wrap gap-1">
                        {names.map((name) => (
                          <span key={name} className="inline-block px-2 py-0.5 rounded bg-bg text-xs text-txt-dim border border-border">
                            {name}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </>
      ) : (
        <p className="text-xs text-txt-dim">
          Click &quot;Detect Packs&quot; to scan for installed DLC and game version.
        </p>
      )}
    </div>
  );
}
