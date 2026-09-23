import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
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
  const connectTo = async (peerId: string, pin?: string) => {
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

  const connectByIp = async (ip: string, port: number, name: string, pin?: string) => {
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

  const leave = async () => {
    setIsLoading(true);
    try {
      await cmd.disconnect();
      setSession(null);
      setIsConnecting(false);
      setDiscoveredPeers([]);
      setSyncPlan(null);
      setSyncProgress(null);
      useAppStore.getState().clearPeerDownloadProgress();
      useAppStore.getState().clearLastHost();
      addLog("Disconnected", "info");
    } catch (e: any) {
      addLog(`Failed to disconnect: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  return { host, join, connectTo, connectByIp, leave, isLoading };
}
