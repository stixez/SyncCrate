import { useCallback, useEffect, useRef, useState } from "react";
import * as cmd from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import type { AutoPullResult } from "../lib/types";

const POLL_MS = 60_000;
const FIRST_PULL_MS = 3_000;

/**
 * "Stay in sync" (client): pull the host's new files every minute and right
 * after connecting. The backend only ever adds non-script files; this keeps
 * the latest result so the dashboard can say what's still waiting.
 */
export function useStayInSync(enabled: boolean) {
  const [last, setLast] = useState<AutoPullResult | null>(null);
  const inFlight = useRef(false);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const addLog = useLogStore((s) => s.addLog);

  const pull = useCallback(async () => {
    const s = useAppStore.getState();
    // Never while the user is mid-flow: a plan on screen, a sync running, or
    // "apply pack exactly" waiting for its download.
    if (!enabledRef.current || inFlight.current || s.syncProgress || s.syncPlan || s.pendingPackApply) return;
    inFlight.current = true;
    try {
      const r = await cmd.autoPull();
      if (!enabledRef.current) return;
      setLast(r);
      if (r.pulled > 0) addLog(`Stay in sync: pulled ${r.pulled} new file${r.pulled !== 1 ? "s" : ""} from the host`, "success");
    } catch (e) {
      addLog(`Stay in sync: ${e}`, "warning");
    } finally {
      inFlight.current = false;
    }
  }, [addLog]);

  useEffect(() => {
    if (!enabled) {
      setLast(null);
      return;
    }
    const first = setTimeout(pull, FIRST_PULL_MS);
    const timer = setInterval(pull, POLL_MS);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [enabled, pull]);

  return { last, pullNow: pull };
}
