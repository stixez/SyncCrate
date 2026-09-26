import { useEffect, useRef } from "react";
import { toastAction, toastError, toastInfo, toastSuccess, toastWithLog } from "../lib/toast";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { BackupProgress, PeerDownloadProgress } from "../lib/types";
import * as cmd from "../lib/commands";
import { runPackApply } from "../lib/packApply";
import { loadDisplayName } from "../lib/prefs";
import { sendNotification } from "../lib/notify";
import { friendlyError } from "../lib/errors";

// One rescan at a time: a change during a scan queues exactly one more.
let scanInFlight = false;
let scanQueued = false;
async function refreshManifest() {
  if (scanInFlight) {
    scanQueued = true;
    return;
  }
  scanInFlight = true;
  try {
    const gameId = useAppStore.getState().selectedGame ?? undefined;
    const manifest = await cmd.scanFiles(gameId);
    if ((useAppStore.getState().selectedGame ?? undefined) === gameId) useAppStore.getState().setManifest(manifest, gameId);
  } catch {
    // Ignore scan failures from the file watcher
  } finally {
    scanInFlight = false;
    if (scanQueued) {
      scanQueued = false;
      refreshManifest();
    }
  }
}

// Host: at most one toast about unreadable files per half minute.
let lastSendErrorToast = 0;
// Host: incoming offers already announced (peer -> file count).
const announcedOffers = new Map<string, number>();

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
    const backupFailureShown = new Set<string>();

    function cancelRetry() {
      useAppStore.getState().setReconnecting(null);
      retryRef.current.active = false;
      if (retryRef.current.timer) {
        clearTimeout(retryRef.current.timer);
        retryRef.current.timer = null;
      }
    }

    // The user's own name. The session's name is the host's label on a
    // client (and already cleared), so reconnects joined as "Guest".
    const reconnectName = () => {
      const attempt = useAppStore.getState().lastConnectAttempt;
      return loadDisplayName().trim() || (attempt && "name" in attempt ? attempt.name : "") || "Guest";
    };

    async function attemptReconnect(hostName: string, localName: string) {
      cancelRetry();
      retryRef.current.active = true;

      const { lastHostIp, lastHostPort, lastHostCode } = useAppStore.getState();
      // A crew join has no code or IP of its own: the stored "last host" is an
      // older, different host. Reconnect through the crew instead.
      const last = useAppStore.getState().lastConnectAttempt;
      const crew = last?.kind === "crew" ? last : null;
      useAppStore.getState().setReconnecting({ host: hostName, attempt: 1, max: MAX_RETRIES });
      // "Stop trying" on the dashboard clears `reconnecting`; checked after
      // every await, and never set again once cleared (a click during an
      // attempt used to be overwritten by the next round).
      const stopped = () => {
        if (!useAppStore.getState().reconnecting) retryRef.current.active = false;
        return !retryRef.current.active || cancelled;
      };

      for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
        if (stopped()) return;

        const delay = RETRY_DELAYS[attempt] || 8000;
        addLog(`Reconnecting in ${delay / 1000}s... (attempt ${attempt + 1}/${MAX_RETRIES})`, "info");
        useAppStore.getState().setReconnecting({ host: hostName, attempt: attempt + 1, max: MAX_RETRIES });

        await new Promise<void>((resolve) => {
          retryRef.current.timer = setTimeout(resolve, delay);
        });

        if (stopped()) return;

        if (crew) {
          try {
            useAppStore.getState().setLastConnectAttempt({ ...crew, name: localName });
            await cmd.connectCrew(crew.crewId, crew.nodeId, localName, crew.pin);
            await new Promise((r) => setTimeout(r, 4000));
            const status = await cmd.getSessionStatus();
            if (status.session_type === "Client" && status.peers.length > 0) {
              setSession(status);
              addLog(`Reconnected to ${hostName}`, "success");
              sendNotification("SyncCrate", `Reconnected to ${hostName}`);
              cancelRetry();
              return;
            }
          } catch {
            // next attempt
          }
          if (stopped()) return;
          continue;
        }

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
              cancelRetry();
              return;
            }
          } catch {
            // Fall through to the other methods
          }
          if (stopped()) return;
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
              cancelRetry();
              return;
            }
            // Connection was attempted but failed (state reset by error handler)
            addLog("Direct IP reconnect failed, trying network scan...", "info");
          } catch {
            addLog("Direct IP reconnect failed, trying network scan...", "info");
          }
          if (stopped()) return;
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
            await new Promise((r) => setTimeout(r, 2000));
            const status = await cmd.getSessionStatus();
            if (status.session_type === "Client" && status.peers.length > 0) {
              setSession(status);
              addLog(`Reconnected to ${hostName}`, "success");
              sendNotification("SyncCrate", `Reconnected to ${hostName}`);
              cancelRetry();
              return;
            }
            // Still handshaking or failed: peer-connected / the next attempt decide.
            if (stopped()) return;
            continue;
          }
          addLog(`Host "${hostName}" not found on network`, "warning");
        } catch {
          addLog(`Reconnect attempt ${attempt + 1} failed`, "warning");
        }
      }

      retryRef.current.active = false;
      useAppStore.getState().setReconnecting(null);
      addLog("Could not reconnect. Please rejoin manually.", "error");
      addLog("If using VPN/Tailscale, make sure the host has allowed SyncCrate through their firewall.", "info");
      sendNotification("SyncCrate", "Connection lost. Could not reconnect.");
    }

    async function setup() {
      const appWindow = getCurrentWebviewWindow();

      const listeners: Promise<UnlistenFn>[] = [
        listen<{ paths: string[]; kind: string }>("files-changed", () => {
          // A running sync writes files constantly; rescan once when it ends
          // (sync-complete) instead of on every burst.
          if (useAppStore.getState().syncProgress) return;
          refreshManifest();
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
          // Read before awaiting: only a client that lost its host reconnects.
          // A host's own friend-dropped events (e.g. right after Stop hosting)
          // made the host "reconnect" to its old join code.
          const wasClient = useAppStore.getState().session?.session_type === "Client";
          // Before the progress is cleared below.
          const midDownload = peer_id ? !!useAppStore.getState().peerDownloadProgress[peer_id]?.file : false;
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
            // Only a log line before, even mid-download.
            if (!wasClient && name && name !== "all") {
              const msg = `${name} lost the connection${midDownload ? " while downloading" : ""}.`;
              toastInfo(msg);
              sendNotification("SyncCrate", msg);
            }
          }

          try {
            const status = await cmd.getSessionStatus();
            setSession(status);
            if (status.session_type === "None") {
              // The plan/progress belonged to the dropped connection; the backend
              // discarded it, so don't leave a stale plan or progress bar behind.
              setSyncPlan(null);
              setSyncProgress(null);
              const st = useAppStore.getState();
              st.setPendingPackApply(null);
              // A tray Disconnect mid-connect never sends connection-failed.
              st.setIsConnecting(false);
              st.setPinPrompt(null);
              // The next session's chat numbers from 1 again.
              st.setChat(null);
              st.clearPeerDownloadProgress();
              for (const id of Object.keys(peerIdleTimers.current)) {
                clearTimeout(peerIdleTimers.current[id]);
                delete peerIdleTimers.current[id];
              }
            }

            // Auto-retry for clients that lost the host
            if (!clean && wasClient && status.session_type === "None") {
              attemptReconnect(name, reconnectName());
            }
          } catch {
            // Session gone — try to reconnect
            if (!clean && wasClient) {
              attemptReconnect(name, reconnectName());
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
          // Not during auto-reconnect: that retries quietly and reports once.
          if (!retryRef.current.active) toastError(msg);
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
        listen<{
          files_synced: number;
          total_bytes: number;
          errors: string[];
          warnings?: string[];
          cancelled?: boolean;
          peer_id?: string;
          changes?: { added: string[]; updated: string[]; removed: string[]; added_count: number; updated_count: number; removed_count: number };
        }>("sync-complete", (event) => {
          setSyncProgress(null);
          // Or the next sync's "Preparing" shows this one's backup counts. Only
          // the presync bar: a manual backup running alongside keeps its own.
          if (useAppStore.getState().backupProgress?.phase === "presync") useAppStore.getState().setBackupProgress(null);
          // `is_syncing` in the stored session is a snapshot; a stale "true"
          // kept Compare & Sync disabled after the sync ended.
          cmd.getSessionStatus().then(setSession).catch(() => {});
          setSyncPlan(null);
          // Watcher events were skipped while the sync wrote files.
          refreshManifest();
          const { files_synced, errors, cancelled } = event.payload;
          const changes = event.payload.changes;
          if (changes && changes.added_count + changes.updated_count + changes.removed_count > 0) {
            useAppStore.getState().setLastSyncChanges({
              ...changes,
              game: useAppStore.getState().activeGame,
              at: Date.now(),
              from: peerName(event.payload.peer_id),
            });
          }
          // Not failures: the files synced, something around them didn't
          // (e.g. file history couldn't keep an old copy).
          const warnings = event.payload.warnings ?? [];
          for (const w of warnings) addLog(w, "warning");
          if (warnings.length > 0) {
            toastWithLog(`${warnings.length} file${warnings.length !== 1 ? "s" : ""} synced without an old copy kept in file history.`);
          }

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
                const problems = event.payload.errors?.length ?? 0;
                toastAction(
                  cancelled
                    ? `Sync cancelled after ${files_synced} file${files_synced !== 1 ? "s" : ""}. Compare again to resume.`
                    : problems ? `Sync finished, but ${problems} file${problems !== 1 ? "s" : ""} couldn't be synced (see the Activity log).` : "Sync complete.",
                  "Undo",
                  () => {
                  cmd.undoLastSync(game).then((r) => {
                    useAppStore.getState().setUndoStatus(null);
                    useAppStore.getState().setLastSyncChanges(null);
                    const parts = [`${r.restored} restored`, `${r.removed} removed`];
                    if (r.skipped.length) parts.push(`${r.skipped.length} skipped`);
                    toastInfo(`Undo: ${parts.join(", ")}`);
                    addLog(`Sync undone: ${parts.join(", ")}`, "info");
                  }).catch((e) => toastError(`Undo failed: ${e}`));
                },
                // The default ~4 s was often gone before anyone read it (undo
                // is also on the Dashboard and the Backups page).
                { duration: 15000 });
              } else if (event.payload.errors?.length) {
                // Nothing arrived, so no Undo toast: say what happened instead.
                const n = event.payload.errors.length;
                toastWithLog(`${n} file${n !== 1 ? "s" : ""} couldn't be synced. The Activity log has the details.`, "error");
              } else if (cancelled) {
                toastInfo("Sync cancelled. Compare again to resume.");
              } else {
                // useSync no longer toasts on its own (it doubled this one).
                toastSuccess("Sync complete.");
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
            // The last event may have been mid-file (a read error ends a
            // file without the "done" event): don't leave "Sending x 40%".
            if (p.file) setPeerDownloadProgress(p.peer_id, { ...p, file: null, file_bytes_sent: 0, file_bytes_total: 0 });
            // "Finished" is reported by the friend now (peer-synced): this
            // guess fired on any pause mid-sync.
          }, PEER_IDLE_MS);
        }),
        listen<{ peer_id: string; name: string; files: number; failed: number }>("peer-synced", (event) => {
          const { name, files, failed } = event.payload;
          const msg = failed > 0
            ? `${name} finished syncing: ${files} file${files !== 1 ? "s" : ""}, ${failed} failed.`
            : `${name} is synced (${files} file${files !== 1 ? "s" : ""}).`;
          addLog(msg, failed > 0 ? "warning" : "success");
          toastInfo(msg);
          sendNotification("SyncCrate", msg);
          cmd.getSessionStatus().then(setSession).catch(() => {});
        }),
        listen("chat-updated", async () => {
          try {
            const prev = useAppStore.getState().chat;
            const next = await cmd.getChat();
            useAppStore.getState().setChat(next);
            // Notify (only when the window is in the background, see
            // sendNotification) about new lines from other people.
            const lastSeen = prev?.messages[prev.messages.length - 1]?.seq ?? 0;
            const me = loadDisplayName().trim().toLowerCase();
            const fresh = next.messages.filter((m) => m.seq > lastSeen && !m.system && m.from.toLowerCase() !== me);
            if (prev && fresh.length > 0) {
              const m = fresh[fresh.length - 1];
              sendNotification(`${m.from} in chat`, m.text);
            }
          } catch {
            // Ignore
          }
        }),
        listen("offers-updated", () => {
          useAppStore.getState().bumpOffersVersion();
          // Host: say when a friend offers files (it only refreshed a panel far down the page).
          if (useAppStore.getState().session?.session_type !== "Host") return;
          cmd.getIncomingOffers().then((offers) => {
            const live = new Set(offers.map((o) => o.peer_id));
            for (const id of [...announcedOffers.keys()]) if (!live.has(id)) announcedOffers.delete(id);
            for (const o of offers) {
              const pending = o.files.filter((f) => f.state === "pending").length;
              // Only when more arrived: each accept/decline also fires this.
              if (pending > (announcedOffers.get(o.peer_id) ?? 0)) {
                const msg = `${o.peer_name} wants to give you ${pending} file${pending !== 1 ? "s" : ""}. See "Files friends want to give you" on the Dashboard.`;
                toastInfo(msg);
                sendNotification("SyncCrate", msg);
              }
              announcedOffers.set(o.peer_id, pending);
            }
          }).catch(() => {});
        }),
        listen<{ peer_id: string; path: string; error: string }>("host-send-error", (event) => {
          const who = peerName(event.payload.peer_id) ?? "A friend";
          addLog(`Couldn't send ${event.payload.path} to ${who}: ${friendlyError(event.payload.error)}`, "warning");
          if (Date.now() - lastSendErrorToast > 30_000) {
            lastSendErrorToast = Date.now();
            toastWithLog(`Couldn't send a file to ${who}: ${friendlyError(event.payload.error)}`, "error");
          }
        }),
        listen("game-info-ready", () => useAppStore.getState().bumpGameInfoVersion()),
        listen("hidden-to-tray", () => {
          // Once: closing looked like quitting, even while hosting.
          try {
            if (localStorage.getItem("synccrate.trayNoticeShown")) return;
            localStorage.setItem("synccrate.trayNoticeShown", "1");
          } catch { /* shows again next time; harmless */ }
          sendNotification("SyncCrate is still running", "It's in the system tray. Right-click its icon to quit.");
        }),
        listen("offer-updated", () => {
          useAppStore.getState().bumpOffersVersion();
        }),
        listen("crews-changed", () => {
          useAppStore.getState().bumpCrewsVersion();
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
        listen<{ game: string; error: string }>("backup-failed", (event) => {
          // Retried every 30 min: say it once per game per run, not each time.
          const { game, error } = event.payload;
          addLog(`Scheduled backup failed: ${error}`, "error");
          if (!backupFailureShown.has(game)) {
            backupFailureShown.add(game);
            toastError(`A scheduled backup failed and will be retried: ${error}`);
          }
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
