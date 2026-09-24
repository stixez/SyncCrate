import { useState, useEffect, type ReactNode } from "react";
import { FolderOpen, RefreshCw, Plus, X, Heart, Coffee, ExternalLink, ImagePlus, RotateCcw, Check, Pipette, Moon, Sun, Monitor } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { toastSuccess, toastError } from "../lib/toast";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { getSyncCount, getTimeSaved } from "../lib/donations";
import { gameLabel, getGameDef } from "../lib/games";
import { GameIcon } from "./Sidebar";
import { Badge, Button, EmptyState, GameArt, Input, LiveDot, Panel, ProgressBar, SectionHeader, Toggle, cx } from "./ui";
import { ACCENT_PRESETS, effectsEnabled, isLightColor } from "../lib/appearance";
import type { Density, ThemeMode, UiScale } from "../lib/prefs";
import { getShowGameArt, invalidateGameArt, setShowGameArt } from "../hooks/useGameArt";
import * as cmd from "../lib/commands";
import { saveGamePath } from "../lib/gamePath";
import type { AutoBackupConfig } from "../lib/types";

export default function Settings() {
  const gamePaths = useAppStore((s) => s.gamePaths);
  const installedGames = useAppStore((s) => s.installedGames);
  const setInstalledGames = useAppStore((s) => s.setInstalledGames);
  const setGamePaths = useAppStore((s) => s.setGamePaths);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const excludePatterns = useAppStore((s) => s.excludePatterns);
  const setExcludePatterns = useAppStore((s) => s.setExcludePatterns);
  const navigateToGlobal = useAppStore((s) => s.navigateToGlobal);
  const addLog = useLogStore((s) => s.addLog);

  const [port, setPort] = useState("9847");
  const [version, setVersion] = useState("");
  const [pathInputs, setPathInputs] = useState<Record<string, string>>({});
  const [updating, setUpdating] = useState(false);
  const [newPattern, setNewPattern] = useState("");
  const [autoBackupConfig, setAutoBackupConfigState] = useState<AutoBackupConfig>({
    auto_backup_before_sync: false,
    auto_backup_scheduled: false,
    auto_backup_interval_hours: 4,
    auto_backup_max_count: 5,
  });
  const [speedLimit, setSpeedLimit] = useState(0);
  const [clearCache, setClearCache] = useState(true);
  const [closeToTray, setCloseToTrayState] = useState(false);
  const [showArt, setShowArtState] = useState(getShowGameArt);
  const [customArt, setCustomArt] = useState<string[]>([]);
  // Saved folders that don't exist right now (kept, not replaced by auto-detect).
  const [unavailable, setUnavailable] = useState<string[]>([]);
  const notificationsEnabled = useAppStore((s) => s.notificationsEnabled);
  const setNotificationsEnabled = useAppStore((s) => s.setNotificationsEnabled);

  // Only show games in the user's library
  const libraryGames = gameRegistry.filter((g) => myLibrary.includes(g.id));

  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
    cmd.getAllGamePaths().then((paths) => {
      const converted: Record<string, string> = {};
      for (const [key, value] of Object.entries(paths)) {
        if (value) converted[key] = value;
      }
      setGamePaths(converted);
      setPathInputs(converted);
    }).catch(() => {});
    cmd.getUnavailableGamePaths().then(setUnavailable).catch(() => {});
    cmd.getExcludePatterns().then(setExcludePatterns).catch(() => {});
    cmd.getTransferSpeedLimit().then(setSpeedLimit).catch(() => {});
    cmd.getClearCacheAfterSync().then(setClearCache).catch(() => {});
    cmd.getCloseToTray().then(setCloseToTrayState).catch(() => {});
  }, [setGamePaths, setExcludePatterns]);

  useEffect(() => {
    cmd.getAutoBackupConfig().then(setAutoBackupConfigState).catch(console.error);
  }, []);

  const updateAutoBackupConfig = (updates: Partial<AutoBackupConfig>) => {
    const newConfig = { ...autoBackupConfig, ...updates };
    setAutoBackupConfigState(newConfig);
    cmd.setAutoBackupConfig(
      newConfig.auto_backup_before_sync,
      newConfig.auto_backup_scheduled,
      newConfig.auto_backup_interval_hours,
      newConfig.auto_backup_max_count,
    ).catch(console.error);
  };

  useEffect(() => {
    cmd.listCustomGameArt().then(setCustomArt).catch(() => {});
  }, []);

  const handleSetCover = async (gameId: string) => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }],
      });
      if (typeof selected !== "string") return;
      await cmd.setCustomGameArt(gameId, selected);
      invalidateGameArt(gameId);
      setCustomArt((prev) => [...new Set([...prev, gameId])]);
      toastSuccess(`${gameLabel(gameId)} cover updated`);
    } catch (e) {
      toastError(String(e));
    }
  };

  const handleResetCover = async (gameId: string) => {
    try {
      await cmd.clearCustomGameArt(gameId);
      invalidateGameArt(gameId);
      setCustomArt((prev) => prev.filter((id) => id !== gameId));
    } catch (e) {
      toastError(String(e));
    }
  };

  // A user-set folder can carry install markers (e.g. Wow.exe for private-server
  // WoW), so re-check install evidence after a path change.
  const refreshInstalled = () => {
    cmd.getInstalledGames().then(setInstalledGames).catch(() => {});
    cmd.getUnavailableGamePaths().then(setUnavailable).catch(() => {});
  };

  /** Save, then show what the backend actually stored (canonical, subfolder corrected). */
  const applyPath = async (gameId: string, path: string) => {
    try {
      const stored = await saveGamePath(gameId, path);
      if (stored === null) {
        // Declined the "doesn't look like a <game> folder" prompt.
        setPathInputs((prev) => ({ ...prev, [gameId]: gamePaths[gameId] ?? "" }));
        return;
      }
      setPathInputs((prev) => ({ ...prev, [gameId]: stored }));
      refreshInstalled();
      addLog(`${gameLabel(gameId)} path updated to: ${stored}`, "success");
      toastSuccess(`${gameLabel(gameId)} path saved`);
    } catch (e) {
      setPathInputs((prev) => ({ ...prev, [gameId]: gamePaths[gameId] ?? "" }));
      addLog(`Failed to set path: ${e}`, "error");
      toastError(`Couldn't use that folder: ${e}`);
    }
  };

  const handleBrowse = async (gameId: string) => {
    const selected = await open({ directory: true }).catch(() => null);
    if (typeof selected === "string") await applyPath(gameId, selected);
  };

  const handlePathSubmit = async (gameId: string) => {
    const input = pathInputs[gameId]?.trim();
    if (!input || input === gamePaths[gameId]) return;
    await applyPath(gameId, input);
  };

  const [portStatus, setPortStatus] = useState<"idle" | "available" | "taken" | "checking">("idle");

  const checkPort = async (value: string) => {
    const p = parseInt(value, 10);
    if (isNaN(p) || p < 1024 || p > 65535) {
      setPortStatus("idle");
      return;
    }
    setPortStatus("checking");
    try {
      const available = await cmd.checkPortAvailable(p);
      setPortStatus(available ? "available" : "taken");
    } catch {
      setPortStatus("idle");
    }
  };

  const handlePortSave = async () => {
    const p = parseInt(port, 10);
    if (isNaN(p) || p < 1024 || p > 65535) {
      addLog("Port must be between 1024 and 65535", "error");
      toastError("Port must be between 1024 and 65535");
      return;
    }
    if (portStatus === "taken") {
      toastError(`Port ${p} is already in use`);
      return;
    }
    try {
      await cmd.setSessionPort(p);
      addLog(`Session port set to ${p}`, "success");
      toastSuccess(`Port saved: ${p}`);
    } catch (e) {
      addLog(`Failed to set port: ${e}`, "error");
      toastError(`Failed to set port`);
    }
  };

  const handleAddPattern = async () => {
    const trimmed = newPattern.trim();
    if (!trimmed || excludePatterns.includes(trimmed)) {
      setNewPattern("");
      return;
    }
    const updated = [...excludePatterns, trimmed];
    try {
      await cmd.setExcludePatterns(updated);
      setExcludePatterns(updated);
      setNewPattern("");
      addLog(`Added sync exclusion: ${trimmed}`, "info");
    } catch (e) {
      addLog(`Failed to add pattern: ${e}`, "error");
    }
  };

  const handleRemovePattern = async (pattern: string) => {
    const updated = excludePatterns.filter((p) => p !== pattern);
    try {
      await cmd.setExcludePatterns(updated);
      setExcludePatterns(updated);
      addLog(`Removed sync exclusion: ${pattern}`, "info");
    } catch (e) {
      addLog(`Failed to remove pattern: ${e}`, "error");
    }
  };

  const selectClass = "input input-sm w-auto pr-8 font-mono text-[12px] cursor-pointer";

  return (
    <div className="max-w-[920px] mx-auto space-y-8 pb-6">
      <SectionHeader
        label={<><b>// Config</b> &nbsp;SyncCrate v{version || "..."}</>}
        title="Settings"
        description="Appearance, game folders, network, backups and app behavior. Changes save as you make them."
      />

      <AppearanceSection />

      <Section num="02" title="Games" description="Where each game in your library keeps its files.">
        {libraryGames.length === 0 ? (
          <EmptyState
            title="No games in your library yet"
            description="Add a game to set its folder here."
            action={
              <Button size="sm" variant="primary" onClick={() => navigateToGlobal("game-browser")} icon={<Plus size={13} />}>
                Browse Games
              </Button>
            }
          />
        ) : (
          <div className="space-y-3">
            {libraryGames.map((game) => {
              const gameDef = getGameDef(game.id);
              const contentFolders = gameDef?.content_types.map((ct) => ct.folder).join(", ") ?? "";
              return (
                <Panel key={game.id} padded={false}>
                  <div className="px-5 py-4 space-y-3">
                    <div className="flex items-center gap-2.5">
                      <span className="relative overflow-hidden w-7 h-7 grid place-items-center border border-line-hi bg-bg shrink-0">
                        <GameArt
                          gameId={game.id}
                          kind="cover"
                          className="absolute inset-0"
                          imgClassName="object-[50%_22%]"
                          fallback={<GameIcon iconName={game.icon} size={14} className={game.color} />}
                        />
                      </span>
                      <h3 className="font-display font-semibold uppercase tracking-[0.05em] text-[14px]">{game.label}</h3>
                      {unavailable.includes(game.id) ? (
                        <Badge tone="amber" dot title="The saved folder is kept; SyncCrate won't scan or sync this game until it's back.">
                          Folder not found — drive disconnected?
                        </Badge>
                      ) : installedGames.includes(game.id) ? (
                        <Badge tone="green" dot>Installed</Badge>
                      ) : gamePaths[game.id] ? (
                        <Badge dot title="The folder is set, but the game itself wasn't found installed on this PC.">Folder found</Badge>
                      ) : (
                        <Badge tone="amber" dot>Not set</Badge>
                      )}
                      <span className="ml-auto flex items-center gap-1">
                        {customArt.includes(game.id) && (
                          <Button variant="ghost" size="sm" onClick={() => handleResetCover(game.id)} icon={<RotateCcw size={12} />} title="Go back to the default art">
                            Reset cover
                          </Button>
                        )}
                        <Button variant="ghost" size="sm" onClick={() => handleSetCover(game.id)} icon={<ImagePlus size={12} />} title="Use your own image for this game">
                          {customArt.includes(game.id) ? "Change cover" : "Custom cover"}
                        </Button>
                      </span>
                    </div>
                    <p className="text-xs text-txt-dim">
                      The root folder for your {game.label} installation
                      {contentFolders && <> (contains <span className="font-mono text-[11px] text-txt-muted">{contentFolders}</span> folders)</>}.
                    </p>
                    <div className="flex gap-2">
                      <Input
                        mono
                        size="sm"
                        value={pathInputs[game.id] || ""}
                        onChange={(e) => setPathInputs((prev) => ({ ...prev, [game.id]: e.target.value }))}
                        onBlur={() => handlePathSubmit(game.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") handlePathSubmit(game.id);
                        }}
                        placeholder={`Path to ${game.label} folder...`}
                        aria-label={`${game.label} folder path`}
                        wrapperClassName="flex-1"
                        className="!text-[12px] !tracking-[0.02em]"
                      />
                      <Button size="sm" onClick={() => handleBrowse(game.id)} icon={<FolderOpen size={13} />} className="!h-[30px]">
                        Browse
                      </Button>
                    </div>
                  </div>
                </Panel>
              );
            })}
          </div>
        )}
      </Section>

      <Section num="03" title="Network & Sync" description="Hosting port and files that never sync.">
        <Panel title="Session port" label="// TCP">
          <p className="text-xs text-txt-dim mb-3">
            Port used for hosting sessions. Change this if the default port (9847) is in use.
          </p>
          <div className="flex gap-2 items-center">
            <input
              type="number"
              value={port}
              onChange={(e) => {
                setPort(e.target.value);
                checkPort(e.target.value);
              }}
              min={1024}
              max={65535}
              aria-label="Session port"
              className={cx(
                "input input-mono w-32",
                portStatus === "taken" && "!border-status-red",
                portStatus === "available" && "!border-status-green",
              )}
            />
            <Button variant="primary" onClick={handlePortSave} disabled={portStatus === "taken"}>
              Save Port
            </Button>
            {portStatus === "checking" && <span className="hud-label">Checking…</span>}
          </div>
          {portStatus === "taken" && (
            <p className="mt-2 font-mono text-[11px] text-status-red">Port is already in use. Choose a different port.</p>
          )}
          {portStatus === "available" && (
            <p className="mt-2 font-mono text-[11px] text-status-green">Port is available.</p>
          )}
        </Panel>

        <Panel title="Sync exclusions" label="// Ignore list">
          <p className="text-xs text-txt-dim mb-3">
            Patterns for files to exclude from sync by default. Use <code className="font-mono text-[11px] text-txt bg-bg border border-border px-1">*.ext</code> for extensions,{" "}
            <code className="font-mono text-[11px] text-txt bg-bg border border-border px-1">folder/*</code> for directories.
          </p>
          {excludePatterns.length > 0 && (
            <div className="bg-bg border border-border divide-y divide-border mb-3">
              {excludePatterns.map((pattern) => (
                <div key={pattern} className="flex items-center gap-2 px-3 h-8 group">
                  <span className="font-mono text-[11px] text-txt-muted">-</span>
                  <code className="font-mono text-[12px] flex-1 text-txt-dim truncate">{pattern}</code>
                  <button
                    onClick={() => handleRemovePattern(pattern)}
                    className="text-txt-muted hover:text-status-red transition-colors"
                    aria-label={`Remove ${pattern}`}
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <Input
              mono
              size="sm"
              value={newPattern}
              onChange={(e) => setNewPattern(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleAddPattern();
              }}
              maxLength={256}
              placeholder="e.g. *.ts4script or Saves/*"
              aria-label="Sync exclusion pattern"
              wrapperClassName="flex-1"
              className="!text-[12px]"
            />
            <Button size="sm" onClick={handleAddPattern} icon={<Plus size={13} />} className="!h-[30px]">
              Add
            </Button>
          </div>
        </Panel>
      </Section>

      <Section num="04" title="Auto-Backups" description="Automatically create backups before syncing or on a schedule.">
        <Panel padded={false}>
          <div className="divide-y divide-border">
            <SettingRow>
              <Toggle
                checked={autoBackupConfig.auto_backup_before_sync}
                onChange={(v) => updateAutoBackupConfig({ auto_backup_before_sync: v })}
                label="Back up before sync"
                description="Snapshot your content folders right before files are received."
              />
            </SettingRow>
            <SettingRow>
              <Toggle
                checked={autoBackupConfig.auto_backup_scheduled}
                onChange={(v) => updateAutoBackupConfig({ auto_backup_scheduled: v })}
                label="Scheduled backups"
                description="Take a backup every few hours while SyncCrate is running."
              />
            </SettingRow>
            {autoBackupConfig.auto_backup_scheduled && (
              <SettingRow label="Backup interval">
                <select
                  value={autoBackupConfig.auto_backup_interval_hours}
                  onChange={(e) => updateAutoBackupConfig({ auto_backup_interval_hours: Number(e.target.value) })}
                  aria-label="Backup interval"
                  className={selectClass}
                >
                  <option value={1}>Every 1 hour</option>
                  <option value={2}>Every 2 hours</option>
                  <option value={4}>Every 4 hours</option>
                  <option value={8}>Every 8 hours</option>
                  <option value={12}>Every 12 hours</option>
                  <option value={24}>Every 24 hours</option>
                </select>
              </SettingRow>
            )}
            {(autoBackupConfig.auto_backup_before_sync || autoBackupConfig.auto_backup_scheduled) && (
              <SettingRow label="Max auto-backups" hint="Oldest auto-backups are removed past this count.">
                <input
                  type="number"
                  value={autoBackupConfig.auto_backup_max_count}
                  min={1}
                  max={20}
                  onChange={(e) => {
                    const val = Math.min(20, Math.max(1, Number(e.target.value)));
                    updateAutoBackupConfig({ auto_backup_max_count: val });
                  }}
                  aria-label="Max auto-backups"
                  className="input input-sm input-mono w-20"
                />
              </SettingRow>
            )}
          </div>
        </Panel>
      </Section>

      <Section num="05" title="Transfer" description="Bandwidth and what happens after a sync finishes.">
        <Panel padded={false}>
          <div className="divide-y divide-border">
            <SettingRow label="Max speed" hint="Limit transfer speed to avoid saturating your network. Unlimited is fastest.">
              <select
                value={speedLimit}
                onChange={(e) => {
                  const val = Number(e.target.value);
                  setSpeedLimit(val);
                  cmd.setTransferSpeedLimit(val).catch(console.error);
                }}
                aria-label="Transfer speed limit"
                className={selectClass}
              >
                <option value={0}>Unlimited</option>
                <option value={1048576}>1 MB/s</option>
                <option value={2097152}>2 MB/s</option>
                <option value={5242880}>5 MB/s</option>
                <option value={10485760}>10 MB/s</option>
                <option value={26214400}>25 MB/s</option>
                <option value={52428800}>50 MB/s</option>
                <option value={104857600}>100 MB/s</option>
              </select>
            </SettingRow>
            <SettingRow>
              <Toggle
                checked={clearCache}
                onChange={(v) => {
                  setClearCache(v);
                  cmd.setClearCacheAfterSync(v).catch(console.error);
                }}
                label="Clear game caches after sync"
                description="For example the Sims 4 localthumbcache, so new CC shows up correctly."
              />
            </SettingRow>
            <SettingRow>
              <Toggle
                checked={notificationsEnabled}
                onChange={setNotificationsEnabled}
                label="Desktop notifications"
                description="When SyncCrate is in the background: sync finished, friend connected."
              />
            </SettingRow>
            <SettingRow>
              <Toggle
                checked={closeToTray}
                onChange={(v) => {
                  setCloseToTrayState(v);
                  cmd.setCloseToTray(v).catch(console.error);
                }}
                label="Keep running in the tray when the window is closed"
                description="Closing the window hides SyncCrate so hosting and syncing continue. Use Quit in the tray menu to exit."
              />
            </SettingRow>
            <SettingRow>
              <Toggle
                checked={showArt}
                onChange={(v) => {
                  setShowArtState(v);
                  setShowGameArt(v);
                }}
                label="Show game art"
                description="Box art and backgrounds from Steam, downloaded once and kept on this PC. Turn off to hide all game art and never contact Steam."
              />
            </SettingRow>
          </div>
        </Panel>
      </Section>

      <Section num="06" title="Application" description="Support the project and keep SyncCrate up to date.">
        <div className="grid grid-cols-2 gap-3 items-stretch">
          <Panel title="Support SyncCrate" label="// Free forever" icon={<Heart size={14} className="text-neon" />}>
            <p className="text-xs text-txt-dim mb-3">
              SyncCrate is free, open-source, and ad-free. One-time support helps keep development going.
            </p>
            {getSyncCount() > 0 && (
              <p className="font-mono text-[11px] text-txt-muted mb-3">
                <span className="text-neon tabular">{getSyncCount()}</span> sync{getSyncCount() !== 1 ? "s" : ""} &middot;{" "}
                <span className="text-neon">{getTimeSaved(getSyncCount())}</span> saved
              </p>
            )}
            <Button
              onClick={() => openUrl("https://www.buymeacoffee.com/stixe").catch(() => {})}
              icon={<Coffee size={14} className="text-amber" />}
              className="group"
            >
              Buy Me a Coffee
              <ExternalLink size={11} className="text-txt-muted" />
            </Button>
          </Panel>

          <Panel title="About" label="// Build">
            <p className="font-display font-bold text-[1.6rem] leading-none tabular">
              v<span className="text-neon">{version || "..."}</span>
            </p>
            <p className="text-xs text-txt-dim mt-2 mb-3">Free and open-source. Licensed under MIT.</p>
            <Button
              onClick={async () => {
                setUpdating(true);
                try {
                  const update = await check();
                  if (update?.available) {
                    const yes = await ask(
                      `Update to v${update.version} is available!\n\n${update.body ?? ""}`,
                      {
                        title: "Update Available",
                        kind: "info",
                        okLabel: "Update",
                        cancelLabel: "Cancel",
                      }
                    );
                    if (yes) {
                      addLog(`Downloading update v${update.version}...`, "info");
                      try { await cmd.disconnect(); } catch {}
                      await update.downloadAndInstall();
                      await relaunch();
                    }
                  } else {
                    await message("You're on the latest version!", {
                      title: "No Update Available",
                      kind: "info",
                      okLabel: "OK",
                    });
                  }
                } catch (e) {
                  addLog(`Update check failed: ${e}`, "error");
                  await message("Failed to check for updates.\nPlease try again later.", {
                    title: "Update Error",
                    kind: "error",
                    okLabel: "OK",
                  });
                } finally {
                  setUpdating(false);
                }
              }}
              disabled={updating}
              icon={<RefreshCw size={14} className={updating ? "animate-spin" : ""} />}
            >
              {updating ? "Checking..." : "Check for Updates"}
            </Button>
          </Panel>
        </div>
      </Section>
    </div>
  );
}

const THEME_OPTIONS: { value: ThemeMode; label: string; icon: ReactNode }[] = [
  { value: "dark", label: "Dark", icon: <Moon size={12} /> },
  { value: "light", label: "Light", icon: <Sun size={12} /> },
  { value: "system", label: "System", icon: <Monitor size={12} /> },
];
const SCALE_OPTIONS: UiScale[] = [0.9, 1, 1.1, 1.25];
const DENSITY_OPTIONS: { value: Density; label: string }[] = [
  { value: "comfortable", label: "Comfortable" },
  { value: "compact", label: "Compact" },
];
const CUSTOM_SWATCH_BG =
  "conic-gradient(from 45deg, #e5484d, #f0a020, #8fcc1a, #1fb87e, #12a8c4, #3d7bfd, #8b5cf6, #e8457e, #e5484d)";

function AppearanceSection() {
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const appearance = useAppStore((s) => s.appearance);
  const setAppearance = useAppStore((s) => s.setAppearance);
  const accent = appearance.accent;
  const preset = ACCENT_PRESETS.find((p) => p.hex === accent);
  const checkClass = (hex: string) => (isLightColor(hex) ? "text-black/80" : "text-white");

  return (
    <Section num="01" title="Appearance" description="Accent color, theme, size and motion. Only affects this PC.">
      <Panel padded={false}>
        <div className="divide-y divide-border">
          <div className="px-5 py-4 flex items-start justify-between gap-5">
            <div className="min-w-0">
              <p className="text-sm text-txt leading-5">Accent color</p>
              <p className="text-xs text-txt-dim mt-0.5">
                {appearance.matchGame ? "Used when no game is selected." : "Highlights, buttons and progress bars."}
              </p>
              <div role="radiogroup" aria-label="Accent color" className="flex flex-wrap gap-2 mt-3">
                {ACCENT_PRESETS.map((p) => (
                  <button
                    key={p.hex}
                    type="button"
                    role="radio"
                    aria-checked={p.hex === accent}
                    aria-label={p.name}
                    title={p.name}
                    className="swatch"
                    onClick={() => setAppearance({ accent: p.hex })}
                  >
                    <span style={{ ["--swatch" as string]: p.hex }}>
                      {p.hex === accent && <Check size={14} strokeWidth={3} className={checkClass(p.hex)} />}
                    </span>
                  </button>
                ))}
                {/* The native picker sits invisibly on top of a swatch so the control keeps the HUD look. */}
                <label className="swatch" role="radio" aria-checked={!preset} title="Custom color">
                  <span style={{ ["--swatch" as string]: preset ? CUSTOM_SWATCH_BG : accent }}>
                    {preset ? (
                      <Pipette size={13} className="text-white drop-shadow-[0_1px_1px_rgb(0_0_0/0.7)]" />
                    ) : (
                      <Check size={14} strokeWidth={3} className={checkClass(accent)} />
                    )}
                  </span>
                  <input
                    type="color"
                    value={accent}
                    onChange={(e) => setAppearance({ accent: e.target.value.toLowerCase() })}
                    aria-label="Custom accent color"
                  />
                </label>
              </div>
              <p className="font-mono text-[11px] text-txt-muted mt-2.5 uppercase tracking-[0.08em]">
                {preset?.name ?? "Custom"} <span className="text-txt-dim">{accent}</span>
              </p>
            </div>
            <AccentPreview />
          </div>
          <SettingRow>
            <Toggle
              checked={appearance.matchGame}
              onChange={(v) => setAppearance({ matchGame: v })}
              label="Match each game's color"
              description="Recolor the app with the selected game's own color instead of your accent."
            />
          </SettingRow>
          <SettingRow label="Theme" hint="System follows your Windows light/dark setting.">
            <Segmented label="Theme" value={theme} onChange={setTheme} options={THEME_OPTIONS} />
          </SettingRow>
          <SettingRow label="UI scale" hint="Make everything larger or smaller.">
            <Segmented
              label="UI scale"
              value={appearance.scale}
              onChange={(v) => setAppearance({ scale: v })}
              options={SCALE_OPTIONS.map((v) => ({ value: v, label: `${Math.round(v * 100)}%` }))}
            />
          </SettingRow>
          <SettingRow label="Density" hint="Compact fits more rows in content lists and the sidebar.">
            <Segmented
              label="Density"
              value={appearance.density}
              onChange={(v) => setAppearance({ density: v })}
              options={DENSITY_OPTIONS}
            />
          </SettingRow>
          <SettingRow>
            <Toggle
              checked={effectsEnabled(appearance)}
              onChange={(v) => setAppearance({ effects: v })}
              label="Visual effects"
              description="Grid backgrounds, scanlines and animations. Off by default when Windows is set to reduce motion."
            />
          </SettingRow>
        </div>
      </Panel>
    </Section>
  );
}

/** Mini HUD panel built from the live tokens, so it shows the accent in the current theme. */
function AccentPreview() {
  return (
    <div aria-hidden className="panel panel-accent w-[216px] shrink-0">
      <div className="p-3.5 space-y-3">
        <div className="flex items-center justify-between">
          <span className="hud-label"><b>// Preview</b></span>
          <Badge tone="neon">Live</Badge>
        </div>
        <div className="flex items-center gap-2">
          <LiveDot />
          <span className="font-display font-semibold uppercase tracking-[0.06em] text-[13px]">Hosting</span>
          <span className="font-mono text-[11px] text-txt-muted ml-auto">3 peers</span>
        </div>
        <ProgressBar value={64} label="Syncing mods" />
        <Button variant="primary" size="sm" block tabIndex={-1}>Sync now</Button>
      </div>
    </div>
  );
}

/** Square HUD segmented control (a radio group). */
function Segmented<T extends string | number>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; icon?: ReactNode }[];
}) {
  return (
    <div role="radiogroup" aria-label={label} className="seg">
      {options.map((o) => (
        <button key={String(o.value)} type="button" role="radio" aria-checked={o.value === value} onClick={() => onChange(o.value)}>
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Two-column settings block: numbered caption + blurb on the left, controls on the right. */
function Section({ num, title, description, children }: { num: string; title: string; description?: string; children: ReactNode }) {
  return (
    <section className="grid grid-cols-[200px_1fr] gap-6 pt-6 border-t border-border first-of-type:border-t-0">
      <div>
        <p className="hud-label mb-1.5"><b>// {num}</b></p>
        <h3 className="font-display font-bold uppercase tracking-[0.04em] text-[1.05rem] leading-tight">{title}</h3>
        {description && <p className="text-xs text-txt-dim mt-2 leading-relaxed">{description}</p>}
      </div>
      <div className="min-w-0 space-y-3">{children}</div>
    </section>
  );
}

/** One row inside a settings panel: a Toggle alone, or a label/hint with a control on the right. */
function SettingRow({ label, hint, children }: { label?: string; hint?: string; children: ReactNode }) {
  if (!label) return <div className="px-5 py-3.5">{children}</div>;
  return (
    <div className="flex items-center justify-between gap-4 px-5 py-3.5">
      <div className="min-w-0">
        <p className="text-sm text-txt leading-5">{label}</p>
        {hint && <p className="text-xs text-txt-dim mt-0.5">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
