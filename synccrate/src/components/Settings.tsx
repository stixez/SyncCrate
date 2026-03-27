import { useState, useEffect } from "react";
import { FolderOpen, RefreshCw, Plus, X, Heart, Coffee, ExternalLink } from "lucide-react";
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
import * as cmd from "../lib/commands";
import type { AutoBackupConfig } from "../lib/types";

export default function Settings() {
  const gamePaths = useAppStore((s) => s.gamePaths);
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
    cmd.getExcludePatterns().then(setExcludePatterns).catch(() => {});
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

  const handleBrowse = async (gameId: string) => {
    try {
      const selected = await open({ directory: true });
      if (selected) {
        const path = typeof selected === "string" ? selected : selected;
        setPathInputs((prev) => ({ ...prev, [gameId]: path }));
        await cmd.setGamePath(gameId, path);
        setGamePaths({ ...gamePaths, [gameId]: path });
        addLog(`${gameLabel(gameId)} path updated to: ${path}`, "success");
        toastSuccess(`${gameLabel(gameId)} path saved`);
      }
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
      toastError(`Failed to set path`);
    }
  };

  const handlePathSubmit = async (gameId: string) => {
    const input = pathInputs[gameId]?.trim();
    if (!input || input === gamePaths[gameId]) return;
    try {
      await cmd.setGamePath(gameId, input);
      setGamePaths({ ...gamePaths, [gameId]: input });
      addLog(`${gameLabel(gameId)} path updated to: ${input}`, "success");
      toastSuccess(`${gameLabel(gameId)} path saved`);
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
      toastError(`Failed to set path`);
    }
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

  return (
    <div className="max-w-xl mx-auto space-y-6">
      <h2 className="text-xl font-bold">Settings</h2>

      <p className="text-xs font-semibold text-txt-dim uppercase tracking-wider mb-2">Game Configuration</p>

      {libraryGames.length === 0 ? (
        <div className="bg-bg-card rounded-xl border border-border p-5 text-center space-y-3">
          <p className="text-sm text-txt-dim">No games in your library yet.</p>
          <button
            onClick={() => navigateToGlobal("game-browser")}
            className="flex items-center gap-1.5 mx-auto px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-sm font-medium transition-colors"
          >
            <Plus size={14} />
            Browse Games
          </button>
        </div>
      ) : (
        libraryGames.map((game) => {
          const gameDef = getGameDef(game.id);
          const contentFolders = gameDef?.content_types.map((ct) => ct.folder).join(", ") ?? "";
          return (
            <div key={game.id} className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
              <div className="flex items-center gap-2">
                <GameIcon iconName={game.icon} size={16} className={game.color} />
                <h3 className="font-semibold text-sm">{game.label} Path</h3>
                {gamePaths[game.id] && (
                  <span className="text-[10px] bg-status-green/20 text-status-green px-1.5 py-0.5 rounded-full font-medium">
                    Detected
                  </span>
                )}
              </div>
              <p className="text-xs text-txt-dim">
                The root folder for your {game.label} installation
                {contentFolders && ` (contains ${contentFolders} folders)`}.
              </p>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={pathInputs[game.id] || ""}
                  onChange={(e) =>
                    setPathInputs((prev) => ({ ...prev, [game.id]: e.target.value }))
                  }
                  onBlur={() => handlePathSubmit(game.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handlePathSubmit(game.id);
                  }}
                  placeholder={`Path to ${game.label} folder...`}
                  aria-label={`${game.label} folder path`}
                  className="flex-1 bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
                />
                <button
                  onClick={() => handleBrowse(game.id)}
                  className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-sm transition-colors"
                >
                  <FolderOpen size={14} />
                  Browse
                </button>
              </div>
            </div>
          );
        })
      )}

      <p className="text-xs font-semibold text-txt-dim uppercase tracking-wider mb-2">Network & Sync</p>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <h3 className="font-semibold text-sm">Network</h3>
        <p className="text-xs text-txt-dim">
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
            className={`w-32 bg-bg border rounded-lg px-3 py-2 text-sm focus:outline-none ${
              portStatus === "taken"
                ? "border-status-red focus:border-status-red"
                : portStatus === "available"
                ? "border-status-green focus:border-status-green"
                : "border-border focus:border-accent"
            }`}
          />
          <button
            onClick={handlePortSave}
            disabled={portStatus === "taken"}
            className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Save Port
          </button>
        </div>
        {portStatus === "taken" && (
          <p className="text-xs text-status-red">Port is already in use. Choose a different port.</p>
        )}
        {portStatus === "available" && (
          <p className="text-xs text-status-green">Port is available.</p>
        )}
      </div>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <h3 className="font-semibold text-sm">Sync Exclusions</h3>
        <p className="text-xs text-txt-dim">
          Patterns for files to exclude from sync by default. Use <code className="bg-bg px-1 rounded">*.ext</code> for extensions, <code className="bg-bg px-1 rounded">folder/*</code> for directories.
        </p>
        <div className="space-y-1.5">
          {excludePatterns.map((pattern) => (
            <div
              key={pattern}
              className="flex items-center gap-2 bg-bg rounded-lg px-3 py-1.5"
            >
              <code className="text-xs flex-1 text-txt-dim">{pattern}</code>
              <button
                onClick={() => handleRemovePattern(pattern)}
                className="text-txt-dim hover:text-status-red transition-colors"
              >
                <X size={14} />
              </button>
            </div>
          ))}
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={newPattern}
            onChange={(e) => setNewPattern(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAddPattern();
            }}
            maxLength={256}
            placeholder="e.g. *.ts4script or Saves/*"
            aria-label="Sync exclusion pattern"
            className="flex-1 bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
          <button
            onClick={handleAddPattern}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-accent hover:bg-accent-light text-white text-sm transition-colors"
          >
            <Plus size={14} />
            Add
          </button>
        </div>
      </div>

      <p className="text-xs font-semibold text-txt-dim uppercase tracking-wider mb-2">Auto-Backups</p>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <h3 className="font-semibold text-sm">Auto-Backups</h3>
        <p className="text-xs text-txt-dim">
          Automatically create backups before syncing or on a schedule.
        </p>
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <label className="text-sm">Back up before sync</label>
            <button
              role="switch"
              aria-checked={autoBackupConfig.auto_backup_before_sync}
              onClick={() => updateAutoBackupConfig({ auto_backup_before_sync: !autoBackupConfig.auto_backup_before_sync })}
              className={`relative w-10 h-6 rounded-full transition-colors ${
                autoBackupConfig.auto_backup_before_sync ? "bg-accent" : "bg-bg border border-border"
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform ${
                  autoBackupConfig.auto_backup_before_sync ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </div>
          <div className="flex items-center justify-between">
            <label className="text-sm">Scheduled backups</label>
            <button
              role="switch"
              aria-checked={autoBackupConfig.auto_backup_scheduled}
              onClick={() => updateAutoBackupConfig({ auto_backup_scheduled: !autoBackupConfig.auto_backup_scheduled })}
              className={`relative w-10 h-6 rounded-full transition-colors ${
                autoBackupConfig.auto_backup_scheduled ? "bg-accent" : "bg-bg border border-border"
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform ${
                  autoBackupConfig.auto_backup_scheduled ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </div>
          {autoBackupConfig.auto_backup_scheduled && (
            <div className="flex items-center justify-between">
              <label className="text-sm">Backup interval</label>
              <select
                value={autoBackupConfig.auto_backup_interval_hours}
                onChange={(e) => updateAutoBackupConfig({ auto_backup_interval_hours: Number(e.target.value) })}
                aria-label="Backup interval"
                className="bg-bg border border-border rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent"
              >
                <option value={1}>Every 1 hour</option>
                <option value={2}>Every 2 hours</option>
                <option value={4}>Every 4 hours</option>
                <option value={8}>Every 8 hours</option>
                <option value={12}>Every 12 hours</option>
                <option value={24}>Every 24 hours</option>
              </select>
            </div>
          )}
          {(autoBackupConfig.auto_backup_before_sync || autoBackupConfig.auto_backup_scheduled) && (
            <div className="flex items-center justify-between">
              <label className="text-sm">Max auto-backups</label>
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
                className="w-20 bg-bg border border-border rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-accent"
              />
            </div>
          )}
        </div>
      </div>

      <p className="text-xs font-semibold text-txt-dim uppercase tracking-wider mb-2">Application</p>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <div className="flex items-center gap-2">
          <Heart size={16} className="text-pink-400" />
          <h3 className="font-semibold text-sm">Support SyncCrate</h3>
        </div>
        <p className="text-xs text-txt-dim">
          SyncCrate is free, open-source, and ad-free. One-time support helps keep development going.
        </p>
        {getSyncCount() > 0 && (
          <p className="text-xs text-txt-dim">
            <span className="text-accent-light font-semibold">{getSyncCount()}</span> sync{getSyncCount() !== 1 ? "s" : ""} — <span className="text-accent-light font-semibold">{getTimeSaved(getSyncCount())}</span> saved
          </p>
        )}
        <button
          onClick={() => openUrl("https://www.buymeacoffee.com/stixe").catch(() => {})}
          className="flex items-center gap-2 px-4 py-2.5 rounded-lg bg-status-yellow/10 hover:bg-status-yellow/20 border border-status-yellow/30 text-sm font-medium transition-colors group"
        >
          <Coffee size={16} className="text-status-yellow" />
          Buy Me a Coffee
          <ExternalLink size={12} className="text-txt-dim opacity-0 group-hover:opacity-100 transition-opacity ml-auto" />
        </button>
      </div>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-3">
        <h3 className="font-semibold text-sm">About</h3>
        <p className="text-sm text-txt-dim">SyncCrate v{version || "..."}</p>
        <p className="text-xs text-txt-dim">
          Free and open-source. Licensed under MIT.
        </p>
        <button
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
          className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-sm transition-colors disabled:opacity-50"
        >
          <RefreshCw size={14} className={updating ? "animate-spin" : ""} />
          {updating ? "Checking..." : "Check for Updates"}
        </button>
      </div>
    </div>
  );
}
