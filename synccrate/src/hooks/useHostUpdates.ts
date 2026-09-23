import { useCallback, useEffect, useRef, useState } from "react";
import * as cmd from "../lib/commands";

const POLL_MS = 60_000;
const FOCUS_MIN_GAP_MS = 15_000;

export interface HostUpdates {
  files: number;
  bytes: number;
}

/**
 * Client side: periodically (every 60 s and when the window regains focus)
 * re-fetch the host's manifest and count files the host has that we don't.
 * Paused while `enabled` is false (not a client, syncing, computing a plan).
 */
export function useHostUpdates(enabled: boolean) {
  const [updates, setUpdates] = useState<HostUpdates | null>(null);
  const inFlight = useRef(false);
  const lastCheck = useRef(0);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  const check = useCallback(async () => {
    if (!enabledRef.current || inFlight.current) return;
    inFlight.current = true;
    lastCheck.current = Date.now();
    try {
      const u = await cmd.checkHostUpdates();
      if (enabledRef.current) setUpdates(u.files > 0 ? u : null);
    } catch {
      // Connection hiccup — the next poll (or disconnect handling) takes over
    } finally {
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      setUpdates(null); // stale once a sync/plan runs; re-check later
      return;
    }
    // Start the clock now: the manifest was just fetched on connect / after a sync.
    lastCheck.current = Date.now();
    const timer = setInterval(check, POLL_MS);
    const onFocus = () => {
      if (Date.now() - lastCheck.current >= FOCUS_MIN_GAP_MS) check();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [enabled, check]);

  const dismiss = useCallback(() => setUpdates(null), []);

  return { updates, dismiss };
}
