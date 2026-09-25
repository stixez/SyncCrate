import { useEffect, useRef } from "react";
import { toastAction, toastError, toastInfo } from "../lib/toast";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { BackupProgress, PeerDownloadProgress } from "../lib/types";
import * as cmd from "../lib/commands";
import { runPackApply } from "../lib/packApply";
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

      const { lastHostIp, lastHostPort, lastHostCode } = useAppStore.getState();

      for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
        if (!retryRef.current.active || cancelled) return;

        const delay = RETRY_DELAYS[attempt] || 8000;
        addLog(`Reconnecting in ${delay / 1000}s... (attempt ${attempt + 1}/${MAX_RETRIES})`, "info");

        await new Promise<void>((resolve) => {
          retryRef.current.timer = setTimeout(resolve, delay);
        });

        if (!retryRef.current.active || cancelled) return;

        // A join code reaches the host on the LAN or over the internet
        if (lastHostCode) {
          try {
            const pin = useAppStore.getState().lastConnectAttempt?.pin;
            useAppStore.getState().setLastConnectAttempt({
              kind: "code", code: lastHostCode, name: localName, label: hostName, pin,
            });
            await cmd.connectByCode(lastHostCode, localName, pin);
            await new Promise((r) => setTimeout(r, 4000));
            const status = await cmd.getSessionStatus();
            if (status.session_type === "Client" && status.peers.length > 0) {
              setSession(status);
              addLog(`Reconnected to ${hostName}`, "success");
              sendNotification("SyncCrate", `Reconnected to ${hostName}`);
              retryRef.current.active = false;
              return;
            }
          } catch {
            // Fall through to the other methods
          }
        }

        // Try direct IP reconnect (works over VPN/Tailscale)
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
          useAppStore.getState().setGameSwitchPrompt(null);
          addLog(`Peer connected: ${event.payload.name}`, "success");
          sendNotification("SyncCrate", `${event.payload.name} connected`);
          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
            // Store host info for direct IP reconnect (clients only)
            if (status.session_type === "Client" && status.peers.length > 0) {
              const host = status.peers[0];
              const attempt = useAppStore.getState().lastConnectAttempt;
              const code = attempt?.kind === "code" ? attempt.code : null;
              // Internet peers report "Internet" instead of an IP — only the
              // join code can reach them again.
              const isIp = /^[0-9a-f.:]+$/i.test(host.ip);
              if (isIp || code) {
                useAppStore.getState().setLastHost(isIp ? host.ip : null, isIp ? host.port : null, host.name, code);
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
        listen<{ message: string }>("internet-unavailable", (event) => {
          addLog(`Internet joining is unavailable: ${event.payload.message}`, "warning");
          toastInfo("Friends outside your network can't join right now — same-network joining still works.");
        }),
        listen<{ message: string }>("discovery-unavailable", (event) => {
          addLog(`LAN auto-discovery is unavailable: ${event.payload.message}`, "warning");
          toastInfo("Auto-discovery couldn't start — friends can still join using Connect by IP.");
        }),
        listen<{ message: string; host_game?: string }>("connection-failed", (event) => {
          const msg = event.payload.message;
          const attempt = useAppStore.getState().lastConnectAttempt;
          if (event.payload.host_game) {
            // The host shares a different game than the one we have selected.
            // Offer a one-click switch instead of a dead-end error.
            cancelRetry();
            addLog(msg, "warning");
            useAppStore.getState().setGameSwitchPrompt({ hostGame: event.payload.host_game, attempt });
            setIsScanning(false);
            useAppStore.getState().setIsConnecting(false);
            setSession(null);
            return;
          }
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

          // "Apply pack exactly": disable/re-enable only after a clean
          // download, so a failed one never leaves mods disabled without
          // the pack's files in their place.
          const pendingApply = useAppStore.getState().pendingPackApply;
          if (pendingApply) {
            useAppStore.getState().setPendingPackApply(null);
            if (pendingApply.preview.game_id !== useAppStore.getState().activeGame) {
              addLog("Apply pack exactly stopped: the active game changed. No mods were disabled.", "warning");
            } else if (cancelled || (errors && errors.length > 0)) {
              const why = cancelled ? "the sync was cancelled" : `the sync had ${errors.length} error(s)`;
              addLog(`Apply pack exactly stopped: ${why}. No mods were disabled; import the pack again to retry.`, "warning");
              toastError(`Pack not applied: ${why}. No mods were disabled.`);
            } else {
              runPackApply(pendingApply.pack, pendingApply.preview);
            }
          }

          // Undo applies to the client only; a host never has a record.
          if (useAppStore.getState().session?.session_type === "Client") {
            const game = useAppStore.getState().activeGame;
            cmd.getUndoStatus(game).then((status) => {
              useAppStore.getState().setUndoStatus(status);
              if (status && (status.added || status.replaced || status.deleted)) {
                toastAction("Sync complete.", "Undo", () => {
                  cmd.undoLastSync(game).then((r) => {
                    useAppStore.getState().setUndoStatus(null);
                    const parts = [`${r.restored} restored`, `${r.removed} removed`];
                    if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
                    toastInfo(`Undo: ${parts.join(", ")}`);
                    addLog(`Sync undone: ${parts.join(", ")}`, "info");
                  }).catch((e) => toastError(`Undo failed: ${e}`));
                });
              }
            }).catch(() => {});
          }

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
        // Backup events (throttled by the backend). Progress goes to the store for
        // BackupList; logging every event flooded the activity log.
        listen<BackupProgress>("backup-progress", (event) => {
          useAppStore.getState().setBackupProgress(event.payload);
        }),
        listen<BackupProgress>("restore-progress", (event) => {
          useAppStore.getState().setBackupProgress(event.payload);
        }),
        // A scheduled backup finished in the background.
        listen("backups-changed", () => {
          cmd.listBackups().then(useAppStore.getState().setBackups).catch(() => {});
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
