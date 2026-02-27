import { useState, useEffect } from "react";
import { FolderOpen, RefreshCw } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";

export default function Settings() {
  const sims4Path = useAppStore((s) => s.sims4Path);
  const setSims4Path = useAppStore((s) => s.setSims4Path);
  const addLog = useLogStore((s) => s.addLog);

  const [port, setPort] = useState("9847");
  const [version, setVersion] = useState("");
  const [pathInput, setPathInput] = useState(sims4Path || "");
  const [updating, setUpdating] = useState(false);

  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
    cmd.getSims4Path().then((p) => {
      setSims4Path(p);
      setPathInput(p);
    }).catch(() => {});
  }, [setSims4Path]);

  const handleBrowse = async () => {
    try {
      const selected = await open({ directory: true });
      if (selected) {
        const path = typeof selected === "string" ? selected : selected;
        setPathInput(path);
        await cmd.setSims4Path(path);
        setSims4Path(path);
        addLog(`Sims 4 path updated to: ${path}`, "success");
      }
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
    }
  };

  const handlePathSubmit = async () => {
    if (!pathInput.trim()) return;
    try {
      await cmd.setSims4Path(pathInput.trim());
      setSims4Path(pathInput.trim());
      addLog(`Sims 4 path updated to: ${pathInput.trim()}`, "success");
    } catch (e) {
      addLog(`Failed to set path: ${e}`, "error");
    }
  };

  const handlePortSave = async () => {
    const p = parseInt(port, 10);
    if (isNaN(p) || p < 1024 || p > 65535) {
      addLog("Port must be between 1024 and 65535", "error");
      return;
    }
    try {
      await cmd.setSessionPort(p);
      addLog(`Session port set to ${p}`, "success");
    } catch (e) {
      addLog(`Failed to set port: ${e}`, "error");
    }
  };

  return (
    <div className="max-w-xl mx-auto space-y-6">
      <h2 className="text-xl font-bold">Settings</h2>

      <div className="bg-bg-card rounded-xl border border-border p-5 space-y-4">
        <h3 className="font-semibold text-sm">Sims 4 Path</h3>
        <p className="text-xs text-txt-dim">
          The root folder for your Sims 4 installation (contains Mods and Saves folders).
        </p>
        <div className="flex gap-2">
          <input
            type="text"
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            placeholder="Path to The Sims 4 folder..."
            className="flex-1 bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
          <button
            onClick={handleBrowse}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg border border-border hover:bg-bg-card-hover text-sm transition-colors"
          >
            <FolderOpen size={14} />
            Browse
          </button>
        </div>
        <button
          onClick={handlePathSubmit}
          className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors"
        >
          Save Path
        </button>
      </div>

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
