import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";

function sendNotification(title: string, body: string) {
  try {
    if ("Notification" in window && Notification.permission === "granted") {
      new Notification(title, { body });
    } else if ("Notification" in window && Notification.permission !== "denied") {
      Notification.requestPermission().then((perm) => {
        if (perm === "granted") new Notification(title, { body });
      });
    }
  } catch {
    // Notifications not supported in this environment
  }
}

export function useTauriEvents() {
  const setSyncProgress = useAppStore((s) => s.setSyncProgress);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const setSession = useAppStore((s) => s.setSession);
  const setManifest = useAppStore((s) => s.setManifest);
  const setIsDragging = useAppStore((s) => s.setIsDragging);
  const addLog = useLogStore((s) => s.addLog);

  const notifRequested = useRef(false);

  useEffect(() => {
    if (!notifRequested.current && "Notification" in window && Notification.permission === "default") {
      notifRequested.current = true;
      Notification.requestPermission().catch(() => {});
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    async function setup() {
      const appWindow = getCurrentWebviewWindow();

      const listeners: Promise<UnlistenFn>[] = [
        listen<{ paths: string[]; kind: string }>("files-changed", async (event) => {
          addLog(`Files changed: ${event.payload.kind}`, "info");
          try {
            const manifest = await cmd.scanFiles();
            setManifest(manifest);
          } catch {
            // Ignore scan failures from file watcher
          }
        }),
        listen<{ name: string }>("peer-connected", async (event) => {
          addLog(`Peer connected: ${event.payload.name}`, "success");
          sendNotification("SimShare", `${event.payload.name} connected`);
          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
          } catch {
            // Ignore if session status fetch fails
          }
        }),
        listen<{ name: string; clean?: boolean; reason?: string }>("peer-disconnected", async (event) => {
          const { name, clean, reason } = event.payload;
          if (clean) {
            addLog(`Peer disconnected: ${name}`, "info");
          } else {
            addLog(`Peer lost: ${name}${reason ? ` (${reason})` : ""}`, "warning");
          }
          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
          } catch {
            // Ignore if session status fetch fails
          }
        }),
        listen<{ message: string }>("connection-failed", (event) => {
          addLog(`Connection failed: ${event.payload.message}`, "error");
          setSession(null);
        }),
        listen<{ file: string; bytes_sent: number; bytes_total: number; files_done: number; files_total: number }>(
          "sync-progress",
          (event) => {
            setSyncProgress(event.payload);
          },
        ),
        listen<{ files_synced: number; total_bytes: number; errors: string[] }>("sync-complete", (event) => {
          setSyncProgress(null);
          setSyncPlan(null);
          const { files_synced, errors } = event.payload;
          if (errors && errors.length > 0) {
            addLog(
              `Sync completed with ${errors.length} error(s): ${files_synced} files synced`,
              "warning",
            );
            for (const err of errors) {
              addLog(`  Sync error: ${err}`, "error");
            }
            sendNotification("SimShare", `Sync completed with ${errors.length} error(s)`);
          } else {
            addLog(`Sync complete: ${files_synced} files synced`, "success");
            sendNotification("SimShare", `Sync complete: ${files_synced} files synced`);
          }
        }),
        listen<{ message: string }>("sync-error", (event) => {
          addLog(`Sync error: ${event.payload.message}`, "error");
        }),
        // Peer game info exchange
        listen<{ peer_id: string }>("peer-game-info", async () => {
          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
          } catch {
            // Ignore
          }
        }),
        // Backup events
        listen<{ file: string; files_done: number; files_total: number }>("backup-progress", (event) => {
          const { files_done, files_total } = event.payload;
          addLog(`Backup progress: ${files_done}/${files_total}`, "info");
        }),
        listen<{ file: string; files_done: number; files_total: number }>("restore-progress", (event) => {
          const { files_done, files_total } = event.payload;
          addLog(`Restore progress: ${files_done}/${files_total}`, "info");
        }),
        // Drag & Drop events
        appWindow.onDragDropEvent((event) => {
          const type = event.payload.type;
          if (type === "enter" || type === "over") {
            setIsDragging(true);
          } else if (type === "drop") {
            setIsDragging(false);
            const payload = event.payload as { type: string; paths?: string[] };
            if (payload.paths && Array.isArray(payload.paths)) {
              window.dispatchEvent(
                new CustomEvent("simshare-drop", { detail: payload.paths }),
              );
            }
          } else if (type === "leave") {
            setIsDragging(false);
          }
        }),
      ];

      const results = await Promise.all(listeners);
      if (!cancelled) {
        unlisteners.push(...results);
      } else {
        results.forEach((fn) => fn());
      }
    }

    setup();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [setSyncProgress, setSyncPlan, setSession, setManifest, setIsDragging, addLog]);
}
