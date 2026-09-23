import { useEffect, useRef } from "react";
import { toastError, toastInfo } from "../lib/toast";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { PeerDownloadProgress } from "../lib/types";
import * as cmd from "../lib/commands";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification as sendOsNotification,
} from "@tauri-apps/plugin-notification";

/** Desktop notification, only when enabled and the window is in the background. */
async function sendNotification(title: string, body: string) {
  try {
    if (!useAppStore.getState().notificationsEnabled) return;
    const win = getCurrentWebviewWindow();
    const [focused, visible] = await Promise.all([win.isFocused(), win.isVisible()]);
    if (focused && visible) return;
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendOsNotification({ title, body });
  } catch {
    // Notifications not supported in this environment
  }
}

function peerName(peerId?: string): string | null {
  if (!peerId) return null;
  return useAppStore.getState().session?.peers.find((p) => p.id === peerId)?.name ?? null;
}

// Host side: the client doesn't announce the end of its sync, so treat a peer
// as done downloading once its progress has been idle for a few seconds.
const PEER_IDLE_MS = 4000;

const MAX_RETRIES = 3;
const RETRY_DELAYS = [2000, 4000, 8000];

export function useTauriEvents() {
  const setSyncProgress = useAppStore((s) => s.setSyncProgress);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const setSession = useAppStore((s) => s.setSession);
  const setManifest = useAppStore((s) => s.setManifest);
  const setIsDragging = useAppStore((s) => s.setIsDragging);
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const setPeerDownloadProgress = useAppStore((s) => s.setPeerDownloadProgress);
  const addLog = useLogStore((s) => s.addLog);

  const peerIdleTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  const retryRef = useRef<{ active: boolean; timer: ReturnType<typeof setTimeout> | null }>({
    active: false,
    timer: null,
  });

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    function cancelRetry() {
      retryRef.current.active = false;
      if (retryRef.current.timer) {
        clearTimeout(retryRef.current.timer);
        retryRef.current.timer = null;
      }
    }

    async function attemptReconnect(hostName: string, localName: string) {
      cancelRetry();
      retryRef.current.active = true;

      const { lastHostIp, lastHostPort } = useAppStore.getState();

      for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
        if (!retryRef.current.active || cancelled) return;

        const delay = RETRY_DELAYS[attempt] || 8000;
        addLog(`Reconnecting in ${delay / 1000}s... (attempt ${attempt + 1}/${MAX_RETRIES})`, "info");

        await new Promise<void>((resolve) => {
          retryRef.current.timer = setTimeout(resolve, delay);
        });

        if (!retryRef.current.active || cancelled) return;

        // Try direct IP reconnect first (works over VPN/Tailscale)
        if (lastHostIp && lastHostPort) {
          try {
            // Reuse the PIN of the attempt that got us connected (if any)
            const pin = useAppStore.getState().lastConnectAttempt?.pin;
            useAppStore.getState().setLastConnectAttempt({
              kind: "ip", ip: lastHostIp, port: lastHostPort, name: localName, label: hostName, pin,
            });
            await cmd.connectByIp(lastHostIp, lastHostPort, localName, pin);
            // connectByIp spawns in background — wait briefly for connection-failed or peer-connected
            await new Promise((r) => setTimeout(r, 2000));
            const status = await cmd.getSessionStatus();
            if (status.session_type === "Client" && status.peers.length > 0) {
              setSession(status);
              addLog(`Reconnected to ${hostName} via direct IP`, "success");
              sendNotification("SyncCrate", `Reconnected to ${hostName}`);
              retryRef.current.active = false;
              return;
            }
            // Connection was attempted but failed (state reset by error handler)
            addLog("Direct IP reconnect failed, trying network scan...", "info");
          } catch {
            addLog("Direct IP reconnect failed, trying network scan...", "info");
          }
        }

        // Fallback: mDNS scan (works on same LAN)
        // Only attempt if session is clean (direct IP error handler may need time to reset)
        try {
          const preStatus = await cmd.getSessionStatus();
          if (preStatus.session_type !== "None") {
            // Session still pending from direct IP attempt — skip mDNS this round
            continue;
          }
          const peers = await cmd.startJoin(localName);
          const match = peers.find((p) => p.name === hostName);
          if (match) {
            const pin = useAppStore.getState().lastConnectAttempt?.pin;
            useAppStore.getState().setLastConnectAttempt({ kind: "peer", peerId: match.id, label: hostName, pin });
            await cmd.connectToPeer(match.id, pin);
            const status = await cmd.getSessionStatus();
            setSession(status);
            addLog(`Reconnected to ${hostName}`, "success");
            sendNotification("SyncCrate", `Reconnected to ${hostName}`);
            retryRef.current.active = false;
            return;
          }
          addLog(`Host "${hostName}" not found on network`, "warning");
        } catch {
          addLog(`Reconnect attempt ${attempt + 1} failed`, "warning");
        }
      }

      retryRef.current.active = false;
      addLog("Could not reconnect. Please rejoin manually.", "error");
      addLog("If using VPN/Tailscale, make sure the host has allowed SyncCrate through their firewall.", "info");
      sendNotification("SyncCrate", "Connection lost. Could not reconnect.");
    }

    async function setup() {
      const appWindow = getCurrentWebviewWindow();

      const listeners: Promise<UnlistenFn>[] = [
        listen<{ paths: string[]; kind: string }>("files-changed", async (event) => {
          addLog(`Files changed: ${event.payload.kind}`, "info");
          try {
            const gameId = useAppStore.getState().selectedGame ?? undefined;
            const manifest = await cmd.scanFiles(gameId);
            if ((useAppStore.getState().selectedGame ?? undefined) === gameId) setManifest(manifest);
          } catch {
            // Ignore scan failures from file watcher
          }
        }),
        listen<{ name: string }>("peer-connected", async (event) => {
          cancelRetry();
          useAppStore.getState().setIsConnecting(false);
          useAppStore.getState().setPinPrompt(null);
          addLog(`Peer connected: ${event.payload.name}`, "success");
          sendNotification("SyncCrate", `${event.payload.name} connected`);
          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
            // Store host info for direct IP reconnect (clients only)
            if (status.session_type === "Client" && status.peers.length > 0) {
              const host = status.peers[0];
              if (host.ip) {
                useAppStore.getState().setLastHost(host.ip, host.port, host.name);
              }
            }
          } catch {
            // Ignore if session status fetch fails
          }
        }),
        listen<{ name: string; clean?: boolean; reason?: string; peer_id?: string }>("peer-disconnected", async (event) => {
          const { name, clean, reason, peer_id } = event.payload;
          setIsScanning(false);
          if (peer_id) {
            setPeerDownloadProgress(peer_id, null);
            clearTimeout(peerIdleTimers.current[peer_id]);
            delete peerIdleTimers.current[peer_id];
          }
          if (clean) {
            cancelRetry(); // Stop any in-progress reconnect attempts
            addLog(`Peer disconnected: ${name}`, "info");
          } else {
            addLog(`Peer lost: ${name}${reason ? ` (${reason})` : ""}`, "warning");
          }

          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
            if (status.session_type === "None") {
              // The plan/progress belonged to the dropped connection; the backend
              // discarded it, so don't leave a stale plan or progress bar behind.
              setSyncPlan(null);
              setSyncProgress(null);
            }

            // Auto-retry for clients that lost the host
            if (!clean && status.session_type === "None") {
              attemptReconnect(name, status.name || "Guest");
            }
          } catch {
            // Session gone — try to reconnect
            if (!clean) {
              attemptReconnect(name, "Guest");
            }
          }
        }),
        listen<{ message: string }>("discovery-unavailable", (event) => {
          addLog(`LAN auto-discovery is unavailable: ${event.payload.message}`, "warning");
          toastInfo("Auto-discovery couldn't start — friends can still join using Connect by IP.");
        }),
        listen<{ message: string }>("connection-failed", (event) => {
          const msg = event.payload.message;
          const attempt = useAppStore.getState().lastConnectAttempt;
          if (/invalid pin/i.test(msg) && attempt) {
            // Host requires a PIN (or ours was wrong): ask for it and retry the
            // exact same attempt instead of failing outright.
            cancelRetry();
            addLog(attempt.pin ? "Wrong PIN — enter the host's current PIN" : "This host requires a PIN", "warning");
            useAppStore.getState().setPinPrompt({ attempt, wrongPin: !!attempt.pin });
            setIsScanning(false);
            useAppStore.getState().setIsConnecting(false);
            setSession(null);
            return;
          }
          addLog(`Connection failed: ${msg}`, "error");
          // The backend now explains the likely cause (firewall timeout vs refused
          // vs unreachable), so surface it directly instead of only in the log.
          toastError(msg);
          if (/forcibly closed|connection reset|10054/i.test(msg)) {
            addLog("The host dropped the connection. Ask the host to click \"Fix Windows Firewall\" in SyncCrate and try again.", "warning");
          }
          setIsScanning(false);
          useAppStore.getState().setIsConnecting(false);
          setSession(null);
        }),
        listen<{ file: string; bytes_sent: number; bytes_total: number; files_done: number; files_total: number }>(
          "sync-progress",
          (event) => {
            setSyncProgress(event.payload);
          },
        ),
        listen<{ files_synced: number; total_bytes: number; errors: string[]; cancelled?: boolean }>("sync-complete", (event) => {
          setSyncProgress(null);
          setSyncPlan(null);
          const { files_synced, errors, cancelled } = event.payload;
          if (cancelled) {
            addLog(`Sync cancelled after ${files_synced} file(s)`, "warning");
            return;
          }
          const from = peerName((event.payload as { peer_id?: string }).peer_id);
          if (errors && errors.length > 0) {
            addLog(
              `Sync completed with ${errors.length} error(s): ${files_synced} files synced`,
              "warning",
            );
            for (const err of errors) {
              addLog(`  Sync error: ${err}`, "error");
            }
            sendNotification("SyncCrate", `Sync finished with ${errors.length} error(s) — see the activity log`);
          } else {
            addLog(`Sync complete: ${files_synced} files synced`, "success");
            sendNotification(
              "SyncCrate",
              `Sync complete — ${files_synced} file${files_synced !== 1 ? "s" : ""}${from ? ` from ${from}` : ""}`,
            );
          }
        }),
        listen<{ message: string }>("sync-error", (event) => {
          addLog(`Sync error: ${event.payload.message}`, "error");
        }),
        // Host sees peer download progress
        listen<PeerDownloadProgress>("peer-download-progress", (event) => {
          const p = event.payload;
          setPeerDownloadProgress(p.peer_id, p);
          const timers = peerIdleTimers.current;
          if (timers[p.peer_id]) clearTimeout(timers[p.peer_id]);
          timers[p.peer_id] = setTimeout(() => {
            delete timers[p.peer_id];
            if (p.files_sent > 0) {
              sendNotification("SyncCrate", `${p.peer_name} finished downloading`);
            }
          }, PEER_IDLE_MS);
        }),
        listen<{ files: string[] }>("caches-cleared", (event) => {
          addLog(`Cleared game caches after sync: ${event.payload.files.join(", ")}`, "info");
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
                new CustomEvent("synccrate-drop", { detail: payload.paths }),
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
      cancelRetry();
      Object.values(peerIdleTimers.current).forEach(clearTimeout);
      peerIdleTimers.current = {};
      unlisteners.forEach((fn) => fn());
    };
  }, [setSyncProgress, setSyncPlan, setSession, setManifest, setIsDragging, setIsScanning, setPeerDownloadProgress, addLog]);
}
