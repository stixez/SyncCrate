import { useState } from "react";
import { useAppStore, type ConnectAttempt } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { SyncFolderPermissions } from "../lib/types";
import * as cmd from "../lib/commands";
import { toastError, toastInfo } from "../lib/toast";

export function useSession() {
  const [isLoading, setIsLoading] = useState(false);
  const setSession = useAppStore((s) => s.setSession);
  const setIsConnecting = useAppStore((s) => s.setIsConnecting);
  const setDiscoveredPeers = useAppStore((s) => s.setDiscoveredPeers);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const setSyncProgress = useAppStore((s) => s.setSyncProgress);
  const addLog = useLogStore((s) => s.addLog);

  const host = async (name: string, usePin?: boolean, allowedFolders?: SyncFolderPermissions) => {
    setIsLoading(true);
    try {
      const info = await cmd.startHost(name, usePin, allowedFolders);
      const status = await cmd.getSessionStatus();
      setSession(status);
      addLog(`Hosting session as "${name}" on port ${info.port}`, "success");
    } catch (e: any) {
      addLog(`Failed to host: ${e}`, "error");
      toastError(`Failed to host: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const join = async (name: string) => {
    setIsLoading(true);
    try {
      const peers = await cmd.startJoin(name);
      setDiscoveredPeers(peers);
      addLog(`Found ${peers.length} host(s) on LAN`, "info");
      if (peers.length === 0) {
        toastInfo("No hosts found. If a friend is hosting, use Connect by IP with an address from their screen.");
      }
    } catch (e: any) {
      addLog(`Failed to scan: ${e}`, "error");
      toastError(`Failed to scan: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  // connectToPeer / connectByIp return immediately — the actual handshake runs in
  // the background. We must NOT mark the session "connected" here; the real session
  // is set when the `peer-connected` event fires (handled in useTauriEvents), and a
  // `connection-failed` event clears the connecting state on failure. Setting it
  // optimistically here previously let users sync into an empty connection
  // ("no active connections").
  const setLastConnectAttempt = useAppStore((s) => s.setLastConnectAttempt);
  const setPinPrompt = useAppStore((s) => s.setPinPrompt);

  const connectTo = async (peerId: string, pin?: string) => {
    const label = useAppStore.getState().discoveredPeers.find((p) => p.id === peerId)?.name ?? "host";
    setLastConnectAttempt({ kind: "peer", peerId, label, pin });
    setPinPrompt(null);
    setIsLoading(true);
    setIsConnecting(true);
    addLog("Connecting to host...", "info");
    try {
      await cmd.connectToPeer(peerId, pin);
    } catch (e: any) {
      addLog(`Failed to connect: ${e}`, "error");
      toastError(`Failed to connect: ${e}`);
      setIsConnecting(false);
    } finally {
      setIsLoading(false);
    }
  };

  const connectByIp = async (ip: string, port: number, name: string, pin?: string, label?: string) => {
    setLastConnectAttempt({ kind: "ip", ip, port, name, label: label ?? `${ip}:${port}`, pin });
    setPinPrompt(null);
    setIsLoading(true);
    setIsConnecting(true);
    addLog(`Connecting to ${ip}:${port}...`, "info");
    try {
      await cmd.connectByIp(ip, port, name, pin);
    } catch (e: any) {
      addLog(`Failed to connect: ${e}`, "error");
      toastError(`Failed to connect: ${e}`);
      setIsConnecting(false);
    } finally {
      setIsLoading(false);
    }
  };

  const connectByCode = async (code: string, name: string, pin?: string) => {
    setLastConnectAttempt({ kind: "code", code, name, label: "host", pin });
    setPinPrompt(null);
    setIsLoading(true);
    setIsConnecting(true);
    addLog("Connecting with join code...", "info");
    try {
      await cmd.connectByCode(code, name, pin);
    } catch (e: any) {
      addLog(`Failed to connect: ${e}`, "error");
      toastError(`Failed to connect: ${e}`);
      setIsConnecting(false);
    } finally {
      setIsLoading(false);
    }
  };

  /** Join a crew member's session by their node id (no code). PIN and
   * wrong-game prompts work exactly like a code join. */
  const connectCrew = async (crewId: string, nodeId: string | undefined, name: string, label: string, pin?: string) => {
    setLastConnectAttempt({ kind: "crew", crewId, nodeId, name, label, pin });
    setPinPrompt(null);
    setIsLoading(true);
    setIsConnecting(true);
    addLog(`Connecting to ${label}...`, "info");
    try {
      await cmd.connectCrew(crewId, nodeId, name, pin);
    } catch (e: any) {
      addLog(`Failed to connect: ${e}`, "error");
      toastError(`Failed to connect: ${e}`);
      setIsConnecting(false);
    } finally {
      setIsLoading(false);
    }
  };

  /** Retry the attempt the host rejected for a missing/wrong PIN, with `pin`. */
  /** Repeat a previous connect attempt, optionally with a (new) PIN. */
  const retryAttempt = async (a: ConnectAttempt, pin?: string) => {
    if (a.kind === "peer") await connectTo(a.peerId, pin);
    else if (a.kind === "ip") await connectByIp(a.ip, a.port, a.name, pin, a.label);
    else if (a.kind === "crew") await connectCrew(a.crewId, a.nodeId, a.name, a.label, pin);
    else await connectByCode(a.code, a.name, pin);
  };

  const retryWithPin = async (pin: string) => {
    const prompt = useAppStore.getState().pinPrompt;
    if (!prompt) return;
    await retryAttempt(prompt.attempt, pin);
  };

  const leave = async () => {
    setIsLoading(true);
    try {
      await cmd.disconnect();
      setSession(null);
      setIsConnecting(false);
      setDiscoveredPeers([]);
      setSyncPlan(null);
      setSyncProgress(null);
      useAppStore.getState().setPendingPackApply(null);
      useAppStore.getState().clearPeerDownloadProgress();
      // Keep the last host so the dashboard can offer "Reconnect to <host>" later
      addLog("Disconnected", "info");
    } catch (e: any) {
      addLog(`Failed to disconnect: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  return { host, join, connectTo, connectByIp, connectByCode, connectCrew, retryWithPin, retryAttempt, leave, isLoading };
}
