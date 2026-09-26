import { useEffect } from "react";
import { useAppStore } from "../stores/useAppStore";
import { useStayInSync } from "../hooks/useStayInSync";

/**
 * Runs "Stay in sync" while connected as a client, whatever page is open. It
 * lived in the Dashboard, so it stopped the moment you went to Content or
 * another game, though the setting promises pulls in the background.
 */
export default function StayInSyncRunner() {
  const on = useAppStore((s) => s.stayInSync);
  const connected = useAppStore((s) => s.session?.session_type === "Client" && (s.session.peers.length ?? 0) > 0);
  const { last } = useStayInSync(on && connected);
  useEffect(() => {
    useAppStore.getState().setAutoPull(last);
  }, [last]);
  return null;
}
