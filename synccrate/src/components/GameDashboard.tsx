import { useState, useEffect, useCallback, useMemo } from "react";
import { Monitor, Users, Package, HardDrive, RefreshCw, AlertTriangle, Lock, Copy, Check, FolderSync, Gamepad2, ChevronDown, ChevronRight, FolderOpen, Settings, Globe } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SyncFolderPermissions, GameInfo, ContentTypeDefinition } from "../lib/types";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { useSession } from "../hooks/useSession";
import { useSync } from "../hooks/useSync";
import { useHostUpdates } from "../hooks/useHostUpdates";
import { loadDisplayName, saveDisplayName, loadUsePin, saveUsePin, loadFolderPerms, saveFolderPerms } from "../lib/prefs";
import { formatBytes } from "../lib/utils";
import { toastSuccess, toastError } from "../lib/toast";
import { getGameDef } from "../lib/games";
import * as cmd from "../lib/commands";
import SyncBanner from "./SyncBanner";
import PeerList from "./PeerList";
import ConnectionGuide from "./ConnectionGuide";
import DonationBanner from "./DonationBanner";
import { FirewallCheck } from "./NetworkHealth";

interface Props {
  gameId: string;
}

export default function GameDashboard({ gameId }: Props) {
  const session = useAppStore((s) => s.session);
  const isConnecting = useAppStore((s) => s.isConnecting);
  const manifest = useAppStore((s) => s.manifest);
  const setManifest = useAppStore((s) => s.setManifest);
  const syncPlan = useAppStore((s) => s.syncPlan);
  const isScanning = useAppStore((s) => s.isScanning);
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const discoveredPeers = useAppStore((s) => s.discoveredPeers);
  const addLog = useLogStore((s) => s.addLog);
  const gamePaths = useAppStore((s) => s.gamePaths);
  const setGamePaths = useAppStore((s) => s.setGamePaths);
  const setPage = useAppStore((s) => s.setPage);
  const activeGame = useAppStore((s) => s.activeGame);
  const lastHostIp = useAppStore((s) => s.lastHostIp);
  const lastHostPort = useAppStore((s) => s.lastHostPort);
  const lastHostName = useAppStore((s) => s.lastHostName);
  const lastHostCode = useAppStore((s) => s.lastHostCode);
  const clearLastHost = useAppStore((s) => s.clearLastHost);
  const pinPrompt = useAppStore((s) => s.pinPrompt);
  const setPinPrompt = useAppStore((s) => s.setPinPrompt);
  const syncProgress = useAppStore((s) => s.syncProgress);
  const { host, join, connectTo, connectByIp, connectByCode, retryWithPin, leave, isLoading } = useSession();
  const { computePlan, executeSync, resolveAll, isLoading: isSyncLoading, loadingPhase } = useSync();

  // "Host has new files" check: clients only, never while syncing/computing a
  // plan or while a plan with pending actions is on screen.
  const hostUpdatesEnabled =
    session?.session_type === "Client" &&
    session.peers.length > 0 &&
    activeGame === gameId &&
    !syncProgress &&
    !isSyncLoading &&
    !(syncPlan && syncPlan.actions.length > 0);
  const { updates: hostUpdates, dismiss: dismissHostUpdates } = useHostUpdates(hostUpdatesEnabled);

  const gameDef = getGameDef(gameId);
  const gameLabel = gameDef?.label ?? gameId;
  const contentTypes = gameDef?.content_types ?? [];

  // Ensure backend active game matches the selected game
  useEffect(() => {
    // Only mirror into the store once the backend accepts it — switching games
    // is refused mid-session, and the store must keep tracking the game that is
    // actually being synced.
    cmd.setActiveGame(gameId)
      .then(() => useAppStore.getState().setActiveGame(gameId))
      .catch((e) => {
        addLog(`Could not switch active game: ${e}`, "warning");
        cmd.getActiveGame().then((g) => useAppStore.getState().setActiveGame(g)).catch(() => {});
      });
  }, [gameId]);

  // Protected install folders (e.g. Sims 4 Game\Bin under Program Files, used by
  // ReShade/GShade) can't be written without elevation — warn up front instead
  // of failing mid-sync.
  const [pathWritable, setPathWritable] = useState(true);
  const [restartingAdmin, setRestartingAdmin] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setPathWritable(true);
    if (!gamePaths[gameId]) return;
    cmd.checkGamePathWritable()
      .then((w) => { if (!cancelled) setPathWritable(w); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [gameId, gamePaths]);

  const writeAccessBanner = !pathWritable && (
    <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-3 flex items-center gap-3">
      <Lock size={16} className="text-status-yellow shrink-0" />
      <p className="flex-1 text-xs text-txt-dim leading-relaxed">
        <span className="font-medium text-txt">SyncCrate can't write to this folder.</span> It's in a protected
        location (like Program Files), so received files can't be saved. Restart SyncCrate as administrator to sync it.
      </p>
      <button
        onClick={async () => {
          setRestartingAdmin(true);
          try { await cmd.restartAsAdmin(); } catch (e) { toastError(String(e)); setRestartingAdmin(false); }
        }}
        disabled={restartingAdmin}
        className="shrink-0 px-3 py-1.5 rounded-lg bg-status-yellow/15 hover:bg-status-yellow/25 text-status-yellow text-xs font-medium transition-colors disabled:opacity-50"
      >
        {restartingAdmin ? "Restarting..." : "Restart as admin"}
      </button>
    </div>
  );

  // Display name, "use PIN" and per-game share permissions are remembered locally
  const [hostName, setHostNameState] = useState(loadDisplayName);
  const setHostName = (name: string) => { setHostNameState(name); saveDisplayName(name); };
  const [usePin, setUsePinState] = useState(loadUsePin);
  const setUsePin = (v: boolean) => { setUsePinState(v); saveUsePin(v); };
  const [folderPerms, setFolderPermsState] = useState<SyncFolderPermissions>({});
  const setFolderPerms = (update: (p: SyncFolderPermissions) => SyncFolderPermissions) => {
    setFolderPermsState((p) => {
      const next = update(p);
      saveFolderPerms(gameId, next);
      return next;
    });
  };
  const [pinCopied, setPinCopied] = useState(false);
  const [pinInput, setPinInput] = useState("");
  useEffect(() => setPinInput(""), [pinPrompt]);
  const [showManualIp, setShowManualIp] = useState(false);
  const [joinCode, setJoinCode] = useState("");
  const [hostJoinCode, setHostJoinCode] = useState<string | null>(null);
  const [joinCodeCopied, setJoinCodeCopied] = useState(false);

  // Fetch the join code whenever we start hosting (port/PIN are baked into it).
  const hostingKey = session?.session_type === "Host" ? `${session.port}:${session.pin ?? ""}` : null;
  useEffect(() => {
    if (!hostingKey) {
      setHostJoinCode(null);
      return;
    }
    let cancelled = false;
    cmd.getJoinCode().then((c) => { if (!cancelled) setHostJoinCode(c); }).catch(() => {});
    return () => { cancelled = true; };
  }, [hostingKey]);
  // Paste a join code anywhere on the dashboard (outside text fields) to fill
  // the join box — people paste codes from chat, nobody types them.
  useEffect(() => {
    if (session && session.session_type !== "None") return;
    const onPaste = (e: ClipboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      const text = e.clipboardData?.getData("text")?.trim() ?? "";
      if (/^SC[-\s]?[0-9A-Z][0-9A-Z\s-]{8,}$/i.test(text)) {
        setJoinCode(text.toUpperCase());
        toastSuccess("Join code pasted — click Join");
      }
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [session]);

  const [manualIp, setManualIp] = useState("");
  const [manualPort, setManualPort] = useState("9847");
  const [manualPin, setManualPin] = useState("");

  const gameInfo = useAppStore((s) => s.gameInfo);
  const setGameInfo = useAppStore((s) => s.setGameInfo);
  const [packsExpanded, setPacksExpanded] = useState(false);
  const [detectingPacks, setDetectingPacks] = useState(false);

  // Initialize folder permissions from content types (all shared), overlaid
  // with this game's saved choices
  useEffect(() => {
    const perms: SyncFolderPermissions = {};
    for (const ct of contentTypes) {
      perms[ct.id] = true;
    }
    const saved = loadFolderPerms(gameId);
    for (const ct of contentTypes) {
      if (typeof saved[ct.id] === "boolean") perms[ct.id] = saved[ct.id];
    }
    setFolderPermsState(perms);
  }, [gameId]);

  const handleDetectPacks = useCallback(async () => {
    setDetectingPacks(true);
    try {
      const info = await cmd.detectPacks(gameId);
      setGameInfo(info);
      const packCount = info?.installed_packs?.length ?? 0;
      toastSuccess(`Detected ${packCount} pack(s)`);
    } catch (e) {
      addLog(`Pack detection failed: ${e}`, "error");
      toastError("Pack detection failed");
    } finally {
      setDetectingPacks(false);
    }
  }, [gameId, setGameInfo, addLog]);

  useEffect(() => {
    cmd.getGameInfo(gameId).then((info) => {
      if (info) setGameInfo(info);
    }).catch(() => {});
  }, [gameId, setGameInfo]);

  const [localVersion, setLocalVersion] = useState("");
  useEffect(() => {
    cmd.getAppVersion().then(setLocalVersion).catch(() => {});
  }, []);

  const isConnected = session && session.session_type !== "None";
  const mismatchedPeers = isConnected && localVersion
    ? session.peers.filter((p) => p.version && p.version !== localVersion)
    : [];

  // Data-driven stat cards from content types
  const statCards = useMemo(() => {
    if (!manifest) return [];
    const files = Object.values(manifest.files);
    const cards: { label: string; value: string | number; color: string }[] = [];

    for (const ct of contentTypes) {
      // Collect all file_type values for this content type
      const types = new Set<string>([ct.file_type]);
      if (ct.classify_by_extension) {
        for (const ft of Object.values(ct.classify_by_extension)) {
          types.add(ft);
        }
      }
      const count = files.filter((f) => types.has(f.file_type)).length;
      cards.push({ label: ct.label, value: count, color: ct.color });
    }

    const totalSize = files.reduce((sum, f) => sum + f.size, 0);
    cards.push({ label: "Total Size", value: formatBytes(totalSize), color: "text-status-yellow" });

    return cards;
  }, [manifest, contentTypes]);

  const handleScan = useCallback(async () => {
    setIsScanning(true);
    try {
      const m = await cmd.scanFiles(gameId);
      // Don't overwrite the manifest if the user switched games mid-scan.
      if (useAppStore.getState().selectedGame !== gameId) return;
      setManifest(m);
      const count = Object.keys(m.files).length;
      toastSuccess(`Scan complete \u2014 ${count} file(s) found`);
    } catch (e) {
      addLog(`Scan failed: ${e}`, "error");
      toastError(`Scan failed: ${e}`);
    } finally {
      setIsScanning(false);
    }
  }, [gameId, setIsScanning, setManifest, addLog]);

  useEffect(() => {
    handleScan();
  }, [gameId]);

  const hasPacks = !!gameDef?.packs;

  if (!isConnected) {
    return (
      <div className="max-w-2xl mx-auto space-y-6">
        <div className="text-center mb-4">
          <h2 className="text-2xl font-bold mb-2">{gameLabel}</h2>
          <p className="text-txt-dim">Sync your files with friends over LAN</p>
        </div>

        {writeAccessBanner}

        {isConnecting && (
          <div className="bg-accent/10 border border-accent/30 rounded-xl p-4 flex items-center gap-3">
            <RefreshCw size={18} className="animate-spin text-accent-light shrink-0" />
            <div>
              <p className="text-sm font-medium">Connecting to host…</p>
              <p className="text-xs text-txt-dim mt-0.5">Finishing the handshake. A large mod folder can take a moment to scan on the host.</p>
            </div>
          </div>
        )}

        {!gamePaths[gameId] && (
          <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-4 flex items-center gap-4">
            <div className="w-10 h-10 rounded-lg bg-status-yellow/20 flex items-center justify-center shrink-0">
              <FolderOpen size={20} className="text-status-yellow" />
            </div>
            <div className="flex-1">
              <p className="text-sm font-medium">Set your {gameLabel} folder</p>
              <p className="text-xs text-txt-dim mt-0.5">SyncCrate needs to know where your game files are.</p>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={async () => {
                  try {
                    const selected = await open({ directory: true });
                    if (selected) {
                      const path = typeof selected === "string" ? selected : selected;
                      await cmd.setGamePath(gameId, path);
                      setGamePaths({ ...gamePaths, [gameId]: path });
                      toastSuccess(`${gameLabel} path saved`);
                      handleScan();
                    }
                  } catch {
                    toastError("Failed to set path");
                  }
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-status-yellow/20 hover:bg-status-yellow/30 text-status-yellow text-sm font-medium transition-colors"
              >
                <FolderOpen size={14} />
                Browse
              </button>
              <button
                onClick={() => useAppStore.getState().navigateToGlobal("settings")}
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
            <p className="text-txt-dim text-sm mb-4">Share your files with others on your network.</p>
            <label className="flex items-center gap-2 mb-3 cursor-pointer text-sm text-txt-dim">
              <input type="checkbox" checked={usePin} onChange={(e) => setUsePin(e.target.checked)} className="rounded border-border accent-accent" />
              <Lock size={14} />
              Require PIN to join
            </label>
            <div className="mb-3">
              <div className="flex items-center gap-1.5 mb-1.5 text-sm text-txt-dim">
                <FolderSync size={14} />
                Shared folders
              </div>
              <div className="grid grid-cols-2 gap-x-3 gap-y-1">
                {contentTypes.filter((ct) => ct.syncable !== false).map((ct) => (
                  <label key={ct.id} className="flex items-center gap-1.5 cursor-pointer text-sm text-txt-dim">
                    <input
                      type="checkbox"
                      checked={folderPerms[ct.id] ?? true}
                      onChange={(e) => setFolderPerms((p) => ({ ...p, [ct.id]: e.target.checked }))}
                      className="rounded border-border accent-accent"
                    />
                    {ct.label}
                  </label>
                ))}
              </div>
            </div>
            <button
              onClick={() => host(hostName.trim() || "Host", usePin, folderPerms)}
              disabled={isLoading || isConnecting}
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
            <p className="text-txt-dim text-sm mb-4">Paste your friend's join code, or scan your network. Join codes work from anywhere.</p>
            {(lastHostCode || (lastHostIp && lastHostPort)) && (
              <div className="mb-3">
                <button
                  onClick={() => {
                    // A PIN-protected host triggers the PIN prompt automatically.
                    // Prefer the join code: it reaches the host over the internet too.
                    if (lastHostCode) {
                      connectByCode(lastHostCode, hostName.trim() || "Guest");
                    } else if (lastHostIp && lastHostPort) {
                      setManualIp(lastHostIp);
                      setManualPort(String(lastHostPort));
                      connectByIp(lastHostIp, lastHostPort, hostName.trim() || "Guest", manualPin || undefined, lastHostName || undefined);
                    }
                  }}
                  disabled={isLoading || isConnecting}
                  className="w-full flex items-center justify-center gap-2 bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50"
                >
                  <RefreshCw size={14} className={isConnecting ? "animate-spin" : ""} />
                  {isConnecting ? "Connecting..." : `Reconnect to ${lastHostName || lastHostIp || "last host"}`}
                </button>
                <p className="text-[11px] text-txt-dim mt-1 flex justify-end gap-2">
                  <button onClick={clearLastHost} className="hover:text-txt shrink-0">Forget</button>
                </p>
              </div>
            )}
            <button
              onClick={() => join(hostName.trim() || "Guest")}
              disabled={isLoading || isConnecting}
              className="w-full bg-status-green/20 hover:bg-status-green/30 text-status-green rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50 mb-3"
            >
              {isLoading ? "Scanning..." : "Scan for Hosts"}
            </button>
            <div className="flex gap-2 mb-3">
              <input
                type="text"
                value={joinCode}
                onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && joinCode.trim()) connectByCode(joinCode, hostName.trim() || "Guest");
                }}
                placeholder="Join code (SC-...)"
                className="flex-1 min-w-0 bg-bg border border-border rounded-lg px-3 py-1.5 text-sm font-mono tracking-wide focus:outline-none focus:border-accent"
              />
              <button
                onClick={() => connectByCode(joinCode, hostName.trim() || "Guest")}
                disabled={!joinCode.trim() || isLoading || isConnecting}
                className="shrink-0 bg-accent/20 hover:bg-accent/30 text-accent-light rounded-lg px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50"
              >
                Join
              </button>
            </div>
            {discoveredPeers.length > 0 && (
              <div className="space-y-2">
                {discoveredPeers.map((peer) => (
                  <button
                    key={peer.id}
                    onClick={() => {
                      if (peer.pin_required) {
                        setPinPrompt({ attempt: { kind: "peer", peerId: peer.id, label: peer.name }, wrongPin: false });
                        setPinInput("");
                      } else {
                        connectTo(peer.id);
                      }
                    }}
                    disabled={isLoading || isConnecting}
                    className="w-full flex items-center justify-between bg-bg rounded-lg px-3 py-2 text-sm hover:bg-bg-card-hover transition-colors disabled:opacity-50"
                  >
                    <span className="flex items-center gap-1.5">
                      {peer.name}
                      {peer.pin_required && <Lock size={12} className="text-txt-dim" />}
                    </span>
                    <span className="flex items-center gap-2">
                      {peer.game_info?.game_version && (
                        <span className="text-accent-light text-[10px] font-medium bg-accent/15 px-1.5 py-0.5 rounded-full">v{peer.game_info.game_version}</span>
                      )}
                      <span className="text-txt-dim text-xs">{peer.mod_count} files</span>
                    </span>
                  </button>
                ))}
              </div>
            )}
            {pinPrompt && (
              <div className={`mt-3 bg-bg rounded-lg border p-3 ${pinPrompt.wrongPin ? "border-status-red/50" : "border-border"}`}>
                <p className="text-sm font-medium mb-0.5">Enter Session PIN</p>
                <p className={`text-xs mb-2 ${pinPrompt.wrongPin ? "text-status-red" : "text-txt-dim"}`}>
                  {pinPrompt.wrongPin
                    ? "That PIN was rejected. Ask the host for the PIN shown on their dashboard."
                    : `${pinPrompt.attempt.label === "host" ? "The host" : pinPrompt.attempt.label} requires a PIN to join.`}
                </p>
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
                      if (e.key === "Enter" && pinInput.length === 4 && !isLoading && !isConnecting) retryWithPin(pinInput);
                      else if (e.key === "Escape") setPinPrompt(null);
                    }}
                  />
                  <button
                    onClick={() => retryWithPin(pinInput)}
                    disabled={pinInput.length !== 4 || isLoading || isConnecting}
                    className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50"
                  >
                    Connect
                  </button>
                </div>
                <button onClick={() => setPinPrompt(null)} className="text-xs text-txt-dim mt-2 hover:text-txt">Cancel</button>
              </div>
            )}
            <div className="mt-3 border-t border-border pt-3">
              <button
                onClick={() => setShowManualIp(!showManualIp)}
                className="flex items-center gap-1.5 text-xs text-txt-dim hover:text-txt transition-colors"
              >
                <Globe size={12} />
                {showManualIp ? "Hide" : "Connect by IP address"}
                {showManualIp ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              </button>
              {showManualIp && (
                <div className="mt-2 space-y-2">
                  <p className="text-[11px] text-txt-dim">For VPN/Tailscale users — enter the host's IP directly. The host must allow SyncCrate through their firewall.</p>
                  <div className="flex gap-2">
                    <input
                      type="text"
                      value={manualIp}
                      onChange={(e) => setManualIp(e.target.value.trim())}
                      placeholder="IP address (e.g. 100.64.1.5)"
                      className="flex-1 bg-bg border border-border rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent"
                    />
                    <input
                      type="text"
                      value={manualPort}
                      onChange={(e) => setManualPort(e.target.value.replace(/\D/g, "").slice(0, 5))}
                      placeholder="9847"
                      className="w-20 bg-bg border border-border rounded-lg px-3 py-1.5 text-sm text-center focus:outline-none focus:border-accent"
                    />
                  </div>
                  <input
                    type="text"
                    inputMode="numeric"
                    maxLength={4}
                    value={manualPin}
                    onChange={(e) => setManualPin(e.target.value.replace(/\D/g, "").slice(0, 4))}
                    placeholder="PIN (optional)"
                    className="w-full bg-bg border border-border rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent"
                  />
                  <button
                    onClick={() => {
                      const port = parseInt(manualPort) || 9847;
                      connectByIp(manualIp, port, hostName.trim() || "Guest", manualPin || undefined);
                    }}
                    disabled={!manualIp || isLoading || isConnecting}
                    className="w-full bg-accent/20 hover:bg-accent/30 text-accent-light rounded-lg px-4 py-1.5 text-sm font-medium transition-colors disabled:opacity-50"
                  >
                    {isLoading || isConnecting ? "Connecting..." : "Connect by IP"}
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>

        {hasPacks && (
          <GameInfoCard
            gameInfo={gameInfo}
            gameLabel={gameLabel}
            packsExpanded={packsExpanded}
            setPacksExpanded={setPacksExpanded}
            detectingPacks={detectingPacks}
            onDetect={handleDetectPacks}
          />
        )}

        {isScanning && !manifest ? (
          <ScanSkeleton />
        ) : manifest ? (
          <StatCardGrid cards={statCards} />
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
          {gamePaths[gameId] && (
            <button
              onClick={() => cmd.openFolder(gamePaths[gameId]!)}
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

  // Connected mode
  const isHost = session.session_type === "Host";
  const isClient = session.session_type === "Client";
  const hostPeer = isClient && session.peers.length > 0 ? session.peers[0] : null;
  // The backend refuses to switch games mid-session, so this page may be showing
  // a different game than the one the session syncs.
  const sessionGameMismatch = activeGame !== gameId;
  const sessionGameLabel = getGameDef(activeGame)?.label ?? activeGame;

  return (
    <div className="space-y-6">
      <DonationBanner />
      {writeAccessBanner}
      {sessionGameMismatch && (
        <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-3 flex items-center gap-3">
          <AlertTriangle size={16} className="text-status-yellow shrink-0" />
          <p className="flex-1 text-xs text-txt-dim leading-relaxed">
            This session is syncing <span className="font-medium text-txt">{sessionGameLabel}</span>. Disconnect to sync {gameLabel} instead.
          </p>
        </div>
      )}
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">{gameLabel} Dashboard</h2>
        <div className="flex gap-2">
          <button onClick={handleScan} disabled={isScanning} className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors disabled:opacity-50">
            <RefreshCw size={14} className={isScanning ? "animate-spin" : ""} />
            Scan Files
          </button>
          {isClient && !sessionGameMismatch && (
            <button onClick={computePlan} disabled={isSyncLoading} className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-sm transition-colors disabled:opacity-50">
              {isSyncLoading ? (loadingPhase || "Computing...") : "Compare & Sync"}
            </button>
          )}
          {gamePaths[gameId] && (
            <button onClick={() => cmd.openFolder(gamePaths[gameId]!)} className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors">
              <FolderOpen size={14} />
              Open Folder
            </button>
          )}
          <button onClick={leave} disabled={isLoading} className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-status-red/20 hover:bg-status-red/30 text-status-red text-sm transition-colors disabled:opacity-50">
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
            onClick={() => { navigator.clipboard.writeText(session.pin!); setPinCopied(true); setTimeout(() => setPinCopied(false), 2000); }}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors"
          >
            {pinCopied ? <Check size={14} className="text-status-green" /> : <Copy size={14} />}
            {pinCopied ? "Copied" : "Copy"}
          </button>
        </div>
      )}

      {isHost && <FirewallCheck compact />}

      {isHost && hostJoinCode && (
        <div className="bg-bg-card rounded-xl border border-accent/30 p-3 flex items-center gap-3">
          <div className="flex-1 min-w-0">
            <p className="text-xs text-txt-dim">
              Join code: copy it and send it to friends. It works on your network and over the internet{session.pin ? ", and includes your PIN" : ""}.
            </p>
            <p className="font-mono text-base font-semibold tracking-wider text-accent-light mt-0.5" title={hostJoinCode}>{shortCode(hostJoinCode)}</p>
          </div>
          <button
            onClick={() => { navigator.clipboard.writeText(hostJoinCode); setJoinCodeCopied(true); setTimeout(() => setJoinCodeCopied(false), 2000); }}
            className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-elevated border border-border hover:border-accent/40 text-sm transition-colors"
          >
            {joinCodeCopied ? <Check size={14} className="text-status-green" /> : <Copy size={14} />}
            {joinCodeCopied ? "Copied" : "Copy"}
          </button>
        </div>
      )}

      {isHost && session.host_ips && session.host_ips.length > 0 && (
        <div className="bg-bg-card rounded-xl border border-border p-3">
          <div className="flex items-center gap-2 mb-2">
            <Globe size={14} className="text-txt-dim" />
            <span className="text-xs text-txt-dim">Share one of these with friends to connect — they'll download your files from here:</span>
          </div>
          <div className="flex flex-wrap gap-2">
            {session.host_ips.map((ip) => (
              <button
                key={ip}
                onClick={() => {
                  navigator.clipboard.writeText(`${ip}:${session.port}`);
                  addLog(`Copied ${ip}:${session.port} to clipboard`, "info");
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-elevated border border-border hover:border-accent/40 text-sm font-mono transition-colors group"
                title="Click to copy"
              >
                <span className="text-txt font-medium">{ip}:{session.port}</span>
                <Copy size={12} className="text-txt-muted group-hover:text-accent-light transition-colors" />
                {ip.startsWith("100.") && <span className="text-[10px] text-accent-light/70 font-sans ml-1">Tailscale</span>}
                {ip.startsWith("10.147.") && <span className="text-[10px] text-blue-400/70 font-sans ml-1">ZeroTier</span>}
              </button>
            ))}
          </div>
        </div>
      )}

      {mismatchedPeers.length > 0 && (
        <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-3 flex items-center gap-2">
          <AlertTriangle size={16} className="text-status-yellow shrink-0" />
          <span className="text-sm text-status-yellow">
            Version mismatch: {mismatchedPeers.map((p) => `${p.name} (v${p.version})`).join(", ")} \u2014 you have v{localVersion}.
          </span>
        </div>
      )}

      {hasPacks && (
        <GameInfoCard gameInfo={gameInfo} gameLabel={gameLabel} packsExpanded={packsExpanded} setPacksExpanded={setPacksExpanded} detectingPacks={detectingPacks} onDetect={handleDetectPacks} />
      )}

      {isClient && hostUpdates && hostUpdates.files > 0 && (
        <div className="bg-accent/10 border border-accent/30 rounded-xl p-3 flex items-center gap-3">
          <Package size={16} className="text-accent-light shrink-0" />
          <p className="flex-1 text-sm">
            Host added <span className="font-semibold">{hostUpdates.files} file{hostUpdates.files !== 1 ? "s" : ""}</span>{" "}
            <span className="text-txt-dim">({formatBytes(hostUpdates.bytes)})</span>
          </p>
          <button
            onClick={() => { dismissHostUpdates(); computePlan(); }}
            disabled={isSyncLoading}
            className="shrink-0 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-xs font-medium transition-colors disabled:opacity-50"
          >
            Compare &amp; Sync
          </button>
          <button onClick={dismissHostUpdates} className="shrink-0 text-xs text-txt-dim hover:text-txt">Dismiss</button>
        </div>
      )}

      {syncPlan && !sessionGameMismatch && syncPlan.actions.length > 0 && (
        <>
          {gameDef?.dangerous_script_extensions && gameDef.dangerous_script_extensions.length > 0 && syncPlan.actions.some((a) => {
            const p = a.ReceiveFromRemote?.relative_path ?? a.Conflict?.remote.relative_path ?? "";
            const ext = p.split(".").pop()?.toLowerCase() ?? "";
            return gameDef.dangerous_script_extensions.includes(ext);
          }) && (
            <div className="bg-status-yellow/10 border border-status-yellow/30 rounded-xl p-3 flex items-center gap-2">
              <AlertTriangle size={16} className="text-status-yellow shrink-0" />
              <span className="text-sm text-status-yellow">This sync includes script files. Only sync from peers you trust.</span>
            </div>
          )}
          {!!syncPlan.resumed_files && syncPlan.resumed_files > 0 && (
            <div className="text-sm text-accent-light">
              Resuming — {syncPlan.resumed_files} files already transferred
            </div>
          )}
          <SyncBanner plan={syncPlan} onSync={executeSync} onResolveAll={resolveAll} />
        </>
      )}
      {syncPlan && !sessionGameMismatch && syncPlan.actions.length === 0 && (
        <div className="bg-status-green/10 border border-status-green/30 rounded-xl p-4 text-center">
          <span className="text-sm text-status-green font-medium">Everything is in sync!</span>
        </div>
      )}

      {isScanning && !manifest ? <ScanSkeleton /> : <StatCardGrid cards={statCards} />}

      <PeerList />
    </div>
  );
}

function StatCardGrid({ cards }: { cards: { label: string; value: string | number; color: string }[] }) {
  return (
    <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
      {cards.map(({ label, value, color }) => (
        <div key={label} className="bg-bg-card rounded-xl border border-border p-4">
          <div className="flex items-center gap-2 mb-2">
            <Package size={16} className={color} />
            <span className="text-txt-dim text-sm">{label}</span>
          </div>
          <p className="text-2xl font-bold">{value}</p>
        </div>
      ))}
    </div>
  );
}

function ScanSkeleton() {
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-center gap-3 py-4">
        <RefreshCw size={18} className="animate-spin text-accent-light" />
        <span className="text-sm text-txt-dim">Scanning files...</span>
      </div>
      <div className="grid grid-cols-2 sm:grid-cols-3 gap-4">
        {Array.from({ length: 4 }).map((_, i) => (
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
  );
}

const PACK_TYPE_LABELS: Record<string, string> = {
  ExpansionPack: "Expansion Packs",
  GamePack: "Game Packs",
  StuffPack: "Stuff Packs",
  Kit: "Kits",
};

function GameInfoCard({ gameInfo, gameLabel, packsExpanded, setPacksExpanded, detectingPacks, onDetect }: {
  gameInfo: GameInfo | null;
  gameLabel: string;
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
          <h3 className="font-semibold text-sm">{gameLabel} Info</h3>
        </div>
        <button onClick={onDetect} disabled={detectingPacks} className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-xs transition-colors disabled:opacity-50">
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
            <span className="text-txt-dim">{packCount} {packCount === 1 ? "pack" : "packs"} detected</span>
          </div>
          {packCount > 0 && (
            <div className="mt-2">
              <button onClick={() => setPacksExpanded(!packsExpanded)} className="flex items-center gap-1 text-xs text-txt-dim hover:text-txt transition-colors">
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
                          <span key={name} className="inline-block px-2 py-0.5 rounded bg-bg text-xs text-txt-dim border border-border">{name}</span>
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
        <p className="text-xs text-txt-dim">Click &quot;Detect Packs&quot; to scan for installed DLC and game version.</p>
      )}
    </div>
  );
}

/** "SC-8M2K-0QRT-…-Q2ZD": enough to recognise a code; Copy puts the full one on the clipboard. */
function shortCode(code: string): string {
  const groups = code.split("-");
  if (groups.length <= 5) return code;
  return `${groups.slice(0, 3).join("-")}-…-${groups[groups.length - 1]}`;
}
