import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";

export function useSession() {
  const [isLoading, setIsLoading] = useState(false);
  const setSession = useAppStore((s) => s.setSession);
  const setDiscoveredPeers = useAppStore((s) => s.setDiscoveredPeers);
  const setSyncPlan = useAppStore((s) => s.setSyncPlan);
  const setSyncProgress = useAppStore((s) => s.setSyncProgress);
  const addLog = useLogStore((s) => s.addLog);

  const host = async (name: string, usePin?: boolean) => {
    setIsLoading(true);
    try {
      const info = await cmd.startHost(name, usePin);
      const status = await cmd.getSessionStatus();
      setSession(status);
      addLog(`Hosting session as "${name}" on port ${info.port}`, "success");
    } catch (e: any) {
      addLog(`Failed to host: ${e}`, "error");
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
    } catch (e: any) {
      addLog(`Failed to scan: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  const connectTo = async (peerId: string, pin?: string) => {
    setIsLoading(true);
    try {
      await cmd.connectToPeer(peerId, pin);
      const status = await cmd.getSessionStatus();
      setSession(status);
      addLog("Connected to host", "success");
    } catch (e: any) {
      addLog(`Failed to connect: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  const leave = async () => {
    setIsLoading(true);
    try {
      await cmd.disconnect();
      setSession(null);
      setDiscoveredPeers([]);
      setSyncPlan(null);
      setSyncProgress(null);
      addLog("Disconnected", "info");
    } catch (e: any) {
      addLog(`Failed to disconnect: ${e}`, "error");
    } finally {
      setIsLoading(false);
    }
  };

  return { host, join, connectTo, leave, isLoading };
}
