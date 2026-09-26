import { useEffect, useState } from "react";
import { Play } from "lucide-react";
import * as cmd from "../lib/commands";
import { toastError, toastInfo } from "../lib/toast";
import { isDemoMode } from "../lib/demoData";
import { Button } from "./ui";

/** Starts the game (through Steam, or The Sims 4's own exe). Hidden when SyncCrate can't. */
export default function PlayButton({ gameId, label }: { gameId: string; label: string }) {
  const [can, setCan] = useState(false);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (isDemoMode()) return;
    cmd.canLaunchGame(gameId).then((c) => { if (!cancelled) setCan(c); }).catch(() => {});
    return () => { cancelled = true; };
  }, [gameId]);

  if (!can) return null;
  return (
    <Button
      size="sm"
      variant="primary"
      disabled={starting}
      icon={<Play size={13} />}
      onClick={async () => {
        setStarting(true);
        try {
          await cmd.launchGame(gameId);
          toastInfo(`Starting ${label}…`);
        } catch (e) {
          toastError(`${e}`);
        } finally {
          // A second click while the launcher opens would start it twice.
          setTimeout(() => setStarting(false), 5000);
        }
      }}
    >
      Play
    </Button>
  );
}
