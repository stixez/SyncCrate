import { useState, useEffect, useCallback, useMemo } from "react";
import { Monitor, Users, Package, RefreshCw, AlertTriangle, Lock, Copy, Check, FolderSync, Gamepad2, ChevronDown, ChevronRight, FolderOpen, Settings, Globe, Power, ArrowDownUp, Radar } from "lucide-react";
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
import { Badge, Banner, Button, Input, LiveDot, Panel, SectionHeader, StatTile, Toggle, cx } from "./ui";
import { isDemoMode, demoJoinCode } from "../lib/demoData";

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
    <Banner
      tone="warn"
      icon={<Lock size={16} />}
      title="SyncCrate can't write to this folder."
      actions={
        <Button
          size="sm"
          variant="secondary"
          onClick={async () => {
            setRestartingAdmin(true);
            try { await cmd.restartAsAdmin(); } catch (e) { toastError(String(e)); setRestartingAdmin(false); }
          }}
          disabled={restartingAdmin}
        >
          {restartingAdmin ? "Restarting..." : "Restart as admin"}
        </Button>
      }
    >
      It's in a protected location (like Program Files), so received files can't be saved. Restart SyncCrate as administrator to sync it.
    </Banner>
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
    cmd.getJoinCode()
      .then((c) => { if (!cancelled) setHostJoinCode(c); })
      .catch(() => { if (!cancelled && isDemoMode()) setHostJoinCode(demoJoinCode); });
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
    const syncableTypes = contentTypes.filter((ct) => ct.syncable !== false);
    return (
      <div className="max-w-[1040px] mx-auto space-y-5">
        <div className="hud-grid -mx-6 -mt-6 px-6 pt-7 pb-2">
          <div className="relative z-[1]">
            <SectionHeader
              size="lg"
              label={<><b>// Session</b> &nbsp;Not connected</>}
              title={gameLabel}
              description="Host a session so friends can pull your files, or join a friend's. Join codes work on your network and over the internet."
              actions={
                <>
                  <Button size="sm" onClick={handleScan} disabled={isScanning} icon={<RefreshCw size={13} className={isScanning ? "animate-spin" : ""} />}>
                    {isScanning ? "Scanning..." : "Scan Files"}
                  </Button>
                  {gamePaths[gameId] && (
                    <Button size="sm" onClick={() => cmd.openFolder(gamePaths[gameId]!)} icon={<FolderOpen size={13} />}>
                      Open Folder
                    </Button>
                  )}
                </>
              }
            />
            {gamePaths[gameId] && (
              <p className="mt-3 font-mono text-[11px] text-txt-muted truncate" title={gamePaths[gameId]!}>
                <span className="text-txt-dim">PATH</span> &nbsp;{gamePaths[gameId]}
              </p>
            )}
          </div>
        </div>

        {writeAccessBanner}

        {isConnecting && (
          <Banner tone="info" icon={<RefreshCw size={16} className="animate-spin" />} title="Connecting to host…">
            Finishing the handshake. A large mod folder can take a moment to scan on the host.
          </Banner>
        )}

        {!gamePaths[gameId] && (
          <Banner
            tone="warn"
            icon={<FolderOpen size={16} />}
            title={`Set your ${gameLabel} folder`}
            actions={
              <>
                <Button
                  size="sm"
                  variant="primary"
                  icon={<FolderOpen size={13} />}
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
                >
                  Browse
                </Button>
                <Button size="sm" variant="ghost" icon={<Settings size={13} />} onClick={() => useAppStore.getState().navigateToGlobal("settings")}>
                  Settings
                </Button>
              </>
            }
          >
            SyncCrate needs to know where your game files are.
          </Banner>
        )}

        <Input
          label="Your name"
          value={hostName}
          onChange={(e) => setHostName(e.target.value.replace(/[^\w\s-]/g, "").slice(0, 32))}
          maxLength={32}
          placeholder="Enter your name..."
          wrapperClassName="max-w-sm"
        />

        <div className="grid grid-cols-2 gap-4 items-start">
          <Panel label={<><b>01</b> &nbsp;Host</>} title="Host a session" icon={<Monitor size={16} className="text-neon" />}>
            <p className="text-txt-dim text-sm mb-5">Friends pull your files. You decide which folders are shared.</p>
            <Toggle
              checked={usePin}
              onChange={setUsePin}
              label="Require PIN to join"
              description="Anyone with your code can join. Posting it publicly? Turn this on."
              className="mb-5"
            />
            <div className="mb-5">
              <p className="hud-label mb-2 flex items-center gap-1.5"><FolderSync size={12} /> Shared folders</p>
              <div className="grid grid-cols-2 gap-x-3 gap-y-2">
                {syncableTypes.map((ct) => (
                  <Toggle
                    key={ct.id}
                    kind="check"
                    checked={folderPerms[ct.id] ?? true}
                    onChange={(v) => setFolderPerms((p) => ({ ...p, [ct.id]: v }))}
                    label={<span className="text-[13px] text-txt-dim">{ct.label}</span>}
                  />
                ))}
              </div>
            </div>
            <Button
              variant="primary"
              size="lg"
              block
              onClick={() => host(hostName.trim() || "Host", usePin, folderPerms)}
              disabled={isLoading || isConnecting}
            >
              {isLoading ? "Starting..." : "Start Hosting"}
            </Button>
          </Panel>

          <Panel label={<><b>02</b> &nbsp;Join</>} title="Join a session" icon={<Users size={16} className="text-neon" />}>
            <p className="text-txt-dim text-sm mb-4">Paste your friend's join code, or scan your network.</p>
            <div className="flex items-stretch gap-2 mb-2">
              <input
                type="text"
                value={joinCode}
                onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && joinCode.trim()) connectByCode(joinCode, hostName.trim() || "Guest");
                }}
                placeholder="SC-XXXX-XXXX-…"
                aria-label="Join code"
                className="input input-mono flex-1 min-w-0 !h-11 text-[15px]"
              />
              <Button
                variant="primary"
                size="lg"
                onClick={() => connectByCode(joinCode, hostName.trim() || "Guest")}
                disabled={!joinCode.trim() || isLoading || isConnecting}
              >
                Join
              </Button>
            </div>
            <p className="font-mono text-[10.5px] text-txt-muted mb-4">Tip: paste a code anywhere on this screen.</p>

            {(lastHostCode || (lastHostIp && lastHostPort)) && (
              <div className="mb-2 flex items-center gap-2">
                <Button
                  block
                  className="flex-1 min-w-0"
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
                  icon={<RefreshCw size={13} className={cx("text-neon", isConnecting && "animate-spin")} />}
                >
                  <span className="truncate">{isConnecting ? "Connecting..." : `Reconnect to ${lastHostName || lastHostIp || "last host"}`}</span>
                </Button>
                <Button variant="ghost" size="sm" onClick={clearLastHost} title="Forget this host">
                  Forget
                </Button>
              </div>
            )}
            <Button block onClick={() => join(hostName.trim() || "Guest")} disabled={isLoading || isConnecting} icon={<Radar size={14} />}>
              {isLoading ? "Scanning..." : "Scan for Hosts"}
            </Button>

            {discoveredPeers.length > 0 && (
              <div className="mt-3 border-t border-border">
                <p className="hud-label pt-3 pb-1.5">Found on your network</p>
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
                    className="group w-full flex items-center justify-between gap-3 px-3 py-2.5 text-sm border-b border-border hover:bg-bg-card-hover transition-colors disabled:opacity-50"
                  >
                    <span className="flex items-center gap-2 min-w-0">
                      <span className="live-dot" />
                      <span className="font-medium truncate">{peer.name}</span>
                      {peer.pin_required && <Lock size={12} className="text-txt-muted shrink-0" />}
                    </span>
                    <span className="flex items-center gap-2 shrink-0">
                      {peer.game_info?.game_version && <Badge tone="neon">v{peer.game_info.game_version}</Badge>}
                      <span className="font-mono text-[11px] text-txt-muted">{peer.mod_count} files</span>
                      <ChevronRight size={14} className="text-txt-muted group-hover:text-neon" />
                    </span>
                  </button>
                ))}
              </div>
            )}

            {pinPrompt && (
              <div className={cx("mt-4 bg-bg border p-3", pinPrompt.wrongPin ? "border-status-red/60" : "border-line-hi")}>
                <p className="hud-label mb-1"><b>PIN</b> &nbsp;required</p>
                <p className={cx("text-xs mb-2.5", pinPrompt.wrongPin ? "text-status-red" : "text-txt-dim")}>
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
                    aria-label="Session PIN"
                    className="input input-mono flex-1 !h-11 text-center text-xl !tracking-[0.5em]"
                    autoFocus
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && pinInput.length === 4 && !isLoading && !isConnecting) retryWithPin(pinInput);
                      else if (e.key === "Escape") setPinPrompt(null);
                    }}
                  />
                  <Button variant="primary" size="lg" onClick={() => retryWithPin(pinInput)} disabled={pinInput.length !== 4 || isLoading || isConnecting}>
                    Connect
                  </Button>
                </div>
                <button onClick={() => setPinPrompt(null)} className="text-xs text-txt-dim mt-2 hover:text-txt">Cancel</button>
              </div>
            )}

            <div className="mt-4 border-t border-border pt-3">
              <button
                onClick={() => setShowManualIp(!showManualIp)}
                aria-expanded={showManualIp}
                className="flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-[0.1em] text-txt-muted hover:text-txt transition-colors"
              >
                <Globe size={12} />
                {showManualIp ? "Hide" : "Connect by IP address"}
                {showManualIp ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              </button>
              {showManualIp && (
                <div className="mt-3 space-y-2">
                  <p className="text-[11px] text-txt-dim">For VPN/Tailscale users — enter the host's IP directly. The host must allow SyncCrate through their firewall.</p>
                  <div className="flex gap-2">
                    <Input
                      mono
                      size="sm"
                      value={manualIp}
                      onChange={(e) => setManualIp(e.target.value.trim())}
                      placeholder="IP address (e.g. 100.64.1.5)"
                      aria-label="Host IP address"
                      wrapperClassName="flex-1 min-w-0"
                    />
                    <Input
                      mono
                      size="sm"
                      value={manualPort}
                      onChange={(e) => setManualPort(e.target.value.replace(/\D/g, "").slice(0, 5))}
                      placeholder="9847"
                      aria-label="Port"
                      wrapperClassName="w-20"
                      className="text-center"
                    />
                  </div>
                  <Input
                    mono
                    size="sm"
                    inputMode="numeric"
                    maxLength={4}
                    value={manualPin}
                    onChange={(e) => setManualPin(e.target.value.replace(/\D/g, "").slice(0, 4))}
                    placeholder="PIN (optional)"
                    aria-label="PIN (optional)"
                  />
                  <Button
                    block
                    size="sm"
                    onClick={() => {
                      const port = parseInt(manualPort) || 9847;
                      connectByIp(manualIp, port, hostName.trim() || "Guest", manualPin || undefined);
                    }}
                    disabled={!manualIp || isLoading || isConnecting}
                  >
                    {isLoading || isConnecting ? "Connecting..." : "Connect by IP"}
                  </Button>
                </div>
              )}
            </div>
          </Panel>
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

  const copyJoinCode = () => {
    if (!hostJoinCode) return;
    navigator.clipboard.writeText(hostJoinCode);
    setJoinCodeCopied(true);
    setTimeout(() => setJoinCodeCopied(false), 2000);
  };

  return (
    <div className="max-w-[1040px] mx-auto space-y-5">
      <DonationBanner />

      <SectionHeader
        label={
          <span className="inline-flex items-center gap-2">
            <LiveDot />
            <b>{isHost ? "Hosting" : "Connected"}</b>
            {session.name && <span>/ {session.name}</span>}
          </span>
        }
        title={gameLabel}
        actions={
          <>
            <Button size="sm" onClick={handleScan} disabled={isScanning} icon={<RefreshCw size={13} className={isScanning ? "animate-spin" : ""} />}>
              Scan Files
            </Button>
            {gamePaths[gameId] && (
              <Button size="sm" onClick={() => cmd.openFolder(gamePaths[gameId]!)} icon={<FolderOpen size={13} />}>
                Open Folder
              </Button>
            )}
            <Button size="sm" variant="danger" onClick={leave} disabled={isLoading} icon={<Power size={13} />}>
              Disconnect
            </Button>
            {isClient && !sessionGameMismatch && (
              <Button variant="primary" onClick={computePlan} disabled={isSyncLoading} icon={<ArrowDownUp size={14} />}>
                {isSyncLoading ? (loadingPhase || "Computing...") : "Compare & Sync"}
              </Button>
            )}
          </>
        }
      />

      {writeAccessBanner}
      {sessionGameMismatch && (
        <Banner tone="warn" icon={<AlertTriangle size={16} />}>
          This session is syncing <span className="font-medium text-txt">{sessionGameLabel}</span>. Disconnect to sync {gameLabel} instead.
        </Banner>
      )}

      {isClient && hostPeer && (
        <Panel tone="accent" padded={false}>
          <div className="flex items-center gap-4 px-5 py-4">
            <div className="cut w-11 h-11 grid place-items-center bg-neon/10 text-neon shrink-0" style={{ ["--cut" as string]: "7px" }}>
              <Monitor size={20} />
            </div>
            <div className="flex-1 min-w-0">
              <p className="hud-label">Connected to</p>
              <p className="font-display font-bold uppercase tracking-[0.04em] text-xl leading-tight truncate">{hostPeer.name}</p>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              {hostPeer.game_info?.game_version && (
                <Badge tone="neon" icon={<Gamepad2 size={10} />}>v{hostPeer.game_info.game_version}</Badge>
              )}
              <span className="font-mono text-xs text-txt-dim">{hostPeer.mod_count} files</span>
              <LiveDot />
            </div>
          </div>
        </Panel>
      )}

      {isHost && (hostJoinCode || session.pin) && (
        <Panel brackets padded={false} className="mx-1">
          <div className="grid lg:grid-cols-[minmax(0,1fr)_auto]">
            {hostJoinCode && (
              <div className="p-5 min-w-0">
                <div className="flex items-center justify-between gap-3 mb-3">
                  <p className="hud-label"><b>// Join code</b> &nbsp;send this to friends</p>
                  <span className="flex items-center gap-2 font-mono text-[10.5px] uppercase tracking-[0.12em] text-neon shrink-0">
                    <LiveDot /> Live
                  </span>
                </div>
                <div className="flex items-stretch gap-2">
                  <button
                    onClick={copyJoinCode}
                    title="Click to copy the full code"
                    className="flex-1 min-w-0 text-left bg-bg border border-line-hi hover:border-neon/70 transition-colors px-4 py-3 font-mono text-[1.15rem] font-medium tracking-[0.06em] text-neon truncate"
                  >
                    {hostJoinCode}
                  </button>
                  <Button
                    variant="primary"
                    size="lg"
                    className="!h-auto min-w-[110px]"
                    onClick={copyJoinCode}
                    icon={joinCodeCopied ? <Check size={16} /> : <Copy size={16} />}
                  >
                    {joinCodeCopied ? "Copied" : "Copy"}
                  </Button>
                </div>
                <p className="text-xs text-txt-dim mt-3">
                  Works on your network and over the internet{session.pin ? ", and includes your PIN" : ""}. No port forwarding needed.
                </p>
              </div>
            )}
            {session.pin && (
              <div className={cx("p-5 flex flex-col justify-between gap-3 lg:min-w-[190px]", hostJoinCode && "border-t lg:border-t-0 lg:border-l border-border")}>
                <p className="hud-label flex items-center gap-1.5"><Lock size={11} /> Session PIN</p>
                <p className="font-display font-bold text-[2.4rem] leading-none tracking-[0.18em] tabular">{session.pin}</p>
                <button
                  onClick={() => { navigator.clipboard.writeText(session.pin!); setPinCopied(true); setTimeout(() => setPinCopied(false), 2000); }}
                  className="self-start flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-[0.1em] text-txt-muted hover:text-neon transition-colors"
                >
                  {pinCopied ? <Check size={12} className="text-neon" /> : <Copy size={12} />}
                  {pinCopied ? "Copied" : "Copy PIN"}
                </button>
              </div>
            )}
          </div>
        </Panel>
      )}

      {isHost && <FirewallCheck compact />}

      {isHost && session.host_ips && session.host_ips.length > 0 && (
        <div className="flex items-center gap-3 flex-wrap">
          <span className="hud-label flex items-center gap-1.5" title="Share one of these with friends to connect — they'll download your files from here">
            <Globe size={12} /> Direct address
          </span>
          {session.host_ips.map((ip) => (
            <button
              key={ip}
              onClick={() => {
                navigator.clipboard.writeText(`${ip}:${session.port}`);
                addLog(`Copied ${ip}:${session.port} to clipboard`, "info");
              }}
              className="group flex items-center gap-2 h-7 px-2.5 bg-bg-card border border-border hover:border-neon/60 font-mono text-xs transition-colors"
              title="Click to copy"
            >
              <span className="text-txt">{ip}:{session.port}</span>
              {ip.startsWith("100.") && <span className="text-[10px] uppercase tracking-[0.08em] text-txt-muted">Tailscale</span>}
              {ip.startsWith("10.147.") && <span className="text-[10px] uppercase tracking-[0.08em] text-txt-muted">ZeroTier</span>}
              <Copy size={11} className="text-txt-muted group-hover:text-neon transition-colors" />
            </button>
          ))}
        </div>
      )}

      {mismatchedPeers.length > 0 && (
        <Banner tone="warn" icon={<AlertTriangle size={16} />} title="Version mismatch">
          {mismatchedPeers.map((p) => `${p.name} (v${p.version})`).join(", ")} — you have v{localVersion}.
        </Banner>
      )}

      {isClient && hostUpdates && hostUpdates.files > 0 && (
        <Banner
          tone="info"
          icon={<Package size={16} />}
          title={<>Host added {hostUpdates.files} file{hostUpdates.files !== 1 ? "s" : ""} <span className="font-mono text-xs text-txt-dim font-normal">({formatBytes(hostUpdates.bytes)})</span></>}
          actions={
            <>
              <Button size="sm" variant="primary" onClick={() => { dismissHostUpdates(); computePlan(); }} disabled={isSyncLoading}>
                Compare &amp; Sync
              </Button>
              <Button size="sm" variant="ghost" onClick={dismissHostUpdates}>Dismiss</Button>
            </>
          }
        />
      )}

      {/* Sync plan area: SyncBanner owns its own styling. */}
      {syncPlan && !sessionGameMismatch && syncPlan.actions.length > 0 && (
        <section className="space-y-3" aria-label="Sync plan">
          {gameDef?.dangerous_script_extensions && gameDef.dangerous_script_extensions.length > 0 && syncPlan.actions.some((a) => {
            const p = a.ReceiveFromRemote?.relative_path ?? a.Conflict?.remote.relative_path ?? "";
            const ext = p.split(".").pop()?.toLowerCase() ?? "";
            return gameDef.dangerous_script_extensions.includes(ext);
          }) && (
            <Banner tone="warn" icon={<AlertTriangle size={16} />}>
              <span className="text-[13px] text-amber">This sync includes script files. Only sync from peers you trust.</span>
            </Banner>
          )}
          {!!syncPlan.resumed_files && syncPlan.resumed_files > 0 && (
            <p className="font-mono text-xs text-neon">
              &gt; Resuming — {syncPlan.resumed_files} files already transferred
            </p>
          )}
          <SyncBanner plan={syncPlan} onSync={executeSync} onResolveAll={resolveAll} />
        </section>
      )}
      {syncPlan && !sessionGameMismatch && syncPlan.actions.length === 0 && (
        <Banner tone="success" icon={<Check size={16} />} title="Everything is in sync" />
      )}

      {hasPacks && (
        <GameInfoCard gameInfo={gameInfo} gameLabel={gameLabel} packsExpanded={packsExpanded} setPacksExpanded={setPacksExpanded} detectingPacks={detectingPacks} onDetect={handleDetectPacks} />
      )}

      {isScanning && !manifest ? <ScanSkeleton /> : <StatCardGrid cards={statCards} />}

      <PeerList />
    </div>
  );
}

function StatCardGrid({ cards }: { cards: { label: string; value: string | number; color: string }[] }) {
  return (
    <section aria-label="Local files">
      <p className="hud-label mb-2.5"><b>//</b> Local files</p>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
        {cards.map(({ label, value }, i) => (
          <StatTile key={label} label={label} value={value} highlight={i === 0} />
        ))}
      </div>
    </section>
  );
}

function ScanSkeleton() {
  return (
    <section aria-label="Scanning files">
      <p className="hud-label mb-2.5 flex items-center gap-2">
        <RefreshCw size={11} className="animate-spin text-neon" /> Scanning files…
      </p>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="bg-bg-card border border-border pl-4 pr-3 py-3">
            <div className="w-16 h-3 mb-3 animate-pulse bg-bg-card-active" />
            <div className="w-12 h-7 animate-pulse bg-bg-card-active" />
          </div>
        ))}
      </div>
    </section>
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
    <Panel
      cut={false}
      label="// Game info"
      title={`${gameLabel}`}
      actions={
        <Button size="sm" onClick={onDetect} disabled={detectingPacks} icon={<RefreshCw size={12} className={detectingPacks ? "animate-spin" : ""} />}>
          {detectingPacks ? "Detecting..." : "Detect Packs"}
        </Button>
      }
    >
      {hasAnyData ? (
        <>
          <div className="flex items-center gap-3 text-sm">
            {hasVersion && <Badge tone="neon">v{gameInfo!.game_version}</Badge>}
            <span className="font-mono text-xs text-txt-dim">{packCount} {packCount === 1 ? "pack" : "packs"} detected</span>
          </div>
          {packCount > 0 && (
            <div className="mt-3">
              <button
                onClick={() => setPacksExpanded(!packsExpanded)}
                aria-expanded={packsExpanded}
                className="flex items-center gap-1 font-mono text-[11px] uppercase tracking-[0.1em] text-txt-muted hover:text-txt transition-colors"
              >
                {packsExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                {packsExpanded ? "Hide packs" : "Show installed packs"}
              </button>
              {packsExpanded && (
                <div className="mt-3 space-y-3">
                  {Object.entries(grouped).map(([type, names]) => (
                    <div key={type}>
                      <p className="hud-label mb-1.5">{PACK_TYPE_LABELS[type] ?? type}</p>
                      <div className="flex flex-wrap gap-1">
                        {names.map((name) => (
                          <span key={name} className="px-2 py-0.5 bg-bg text-xs text-txt-dim border border-border">{name}</span>
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
    </Panel>
  );
}
