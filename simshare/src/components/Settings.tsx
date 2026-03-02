import { useState, useEffect } from "react";
import { FolderOpen, RefreshCw, Plus, X, ChevronDown, Heart, Coffee, ExternalLink } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { toastSuccess, toastError } from "../lib/toast";
import type { SimsGame } from "../lib/types";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { getSyncCount, getTimeSaved } from "../lib/donations";
import * as cmd from "../lib/commands";

const GAMES: { key: SimsGame; label: string }[] = [
  { key: "Sims2", label: "Sims 2" },
  { key: "Sims3", label: "Sims 3" },
  { key: "Sims4", label: "Sims 4" },
];

export default function Settings() {
  const gamePaths = useAppStore((s) => s.gamePaths);
  const setGamePaths = useAppStore((s) => s.setGamePaths);
  const activeGame = useAppStore((s) => s.activeGame);
  const setActiveGame = useAppStore((s) => s.setActiveGame);
  const excludePatterns = useAppStore((s) => s.excludePatterns);
  const setExcludePatterns = useAppStore((s) => s.setExcludePatterns);
  const addLog = useLogStore((s) => s.addLog);

  const [port, setPort] = useState("9847");
  const [version, setVersion] = useState("");
  const [pathInputs, setPathInputs] = useState<Partial<Record<SimsGame, string>>>({});
  const [updating, setUpdating] = useState(false);
  const [newPattern, setNewPattern] = useState("");

  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
    cmd.getAllGamePaths().then((paths) => {
      const converted: Partial<Record<SimsGame, string>> = {};
      for (const [key, value] of Object.entries(paths)) {
        if (value) converted[key as SimsGame] = value;
      }
      setGamePaths(converted);
      setPathInputs(converted);
    }).catch(() => {});
    cmd.getActiveGame().then((g) => setActiveGame(g as SimsGame)).catch(() => {});
    cmd.getExcludePatterns().then(setExcludePatterns).catch(() => {});
  }, [setGamePaths, setActiveGame, setExcludePatterns]);

  const handleBrowse = async (game: SimsGame) => {
    try {
      const selected = await open({ directory: true });
      if (selected) {
        const path = typeof selected === "string" ? selected : selected;
        setPathInputs((prev) => ({ ...prev, [game]: path }));
        await cmd.setGamePath(game, path);
        setGamePaths({ ...gamePaths, [game]: path });
        addLog(`${GAMES.find((g) => g.key === game)?.label} path updated to: ${path}`, "success");
        toastSuccess(`${GAMES.find((g) => g.key === game)?.label} path saved`);
      }
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
      toastError(`Failed to set path`);
    }
  };

  const handlePathSubmit = async (game: SimsGame) => {
    const input = pathInputs[game]?.trim();
    if (!input || input === gamePaths[game]) return;
    try {
      await cmd.setGamePath(game, input);
      setGamePaths({ ...gamePaths, [game]: input });
      addLog(`${GAMES.find((g) => g.key === game)?.label} path updated to: ${input}`, "success");
      toastSuccess(`${GAMES.find((g) => g.key === game)?.label} path saved`);
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
      toastError(`Failed to set path`);
    }
  };

  const handleActiveGameChange = async (game: SimsGame) => {
    try {
      await cmd.setActiveGame(game);
      setActiveGame(game);
      // Invalidate manifest so Dashboard triggers a fresh scan for the new game
      useAppStore.getState().setManifest(null);
      addLog(`Active game set to ${GAMES.find((g) => g.key === game)?.label}`, "info");
      toastSuccess(`Switched to ${GAMES.find((g) => g.key === game)?.label}`);
    } catch (e) {
      addLog(`Failed to set active game: ${e}`, "error");
      toastError(`Failed to switch game`);
    }
  };

  const handlePortSave = async () => {
    const p = parseInt(port, 10);
    if (isNaN(p) || p < 1024 || p > 65535) {
      addLog("Port must be between 1024 and 65535", "error");
      toastError("Port must be between 1024 and 65535");
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

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <h3 className="font-semibold text-sm">Active Game</h3>
        <p className="text-xs text-txt-dim">
          Select which game SimShare should scan, sync, backup, and install for.
        </p>
        <div className="flex gap-2">
          {GAMES.map(({ key, label }) => (
            <button
              key={key}
              onClick={() => handleActiveGameChange(key)}
              className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors border ${
                activeGame === key
                  ? "bg-accent text-white border-accent"
                  : "bg-bg border-border text-txt-dim hover:bg-bg-card-hover"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {GAMES.map(({ key, label }) => (
        <div key={key} className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
          <div className="flex items-center gap-2">
            <h3 className="font-semibold text-sm">{label} Path</h3>
            {gamePaths[key] && (
              <span className="text-[10px] bg-status-green/20 text-status-green px-1.5 py-0.5 rounded-full font-medium">
                Detected
              </span>
            )}
            {key === activeGame && (
              <span className="text-[10px] bg-accent/20 text-accent-light px-1.5 py-0.5 rounded-full font-medium">
                Active
              </span>
            )}
          </div>
          <p className="text-xs text-txt-dim">
            The root folder for your {label} installation (contains Mods and Saves folders).
          </p>
          <div className="flex gap-2">
            <input
              type="text"
              value={pathInputs[key] || ""}
              onChange={(e) =>
                setPathInputs((prev) => ({ ...prev, [key]: e.target.value }))
              }
              onBlur={() => handlePathSubmit(key)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handlePathSubmit(key);
              }}
              placeholder={`Path to The ${label} folder...`}
              aria-label={`${label} folder path`}
              className="flex-1 bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
            />
            <button
              onClick={() => handleBrowse(key)}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-sm transition-colors"
            >
              <FolderOpen size={14} />
              Browse
            </button>
          </div>
        </div>
      ))}

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
            onChange={(e) => setPort(e.target.value)}
            min={1024}
            max={65535}
            aria-label="Session port"
            className="w-32 bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
          <button
            onClick={handlePortSave}
            className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors"
          >
            Save Port
          </button>
        </div>
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

      <p className="text-xs font-semibold text-txt-dim uppercase tracking-wider mb-2">Application</p>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <div className="flex items-center gap-2">
          <Heart size={16} className="text-pink-400" />
          <h3 className="font-semibold text-sm">Support SimShare</h3>
        </div>
        <p className="text-xs text-txt-dim">
          SimShare is free, open-source, and ad-free. One-time support helps keep development going.
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
        <p className="text-sm text-txt-dim">SimShare v{version || "..."}</p>
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
