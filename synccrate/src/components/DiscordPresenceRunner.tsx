import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import { plural } from "../lib/utils";
import { DISCORD_PREF_EVENT, loadDiscordPresence } from "../lib/prefs";
import * as cmd from "../lib/commands";

/**
 * Shows the running session on the user's Discord profile ("Hosting The
 * Sims 4 · 2 friends connected"), whatever page is open. Only the game and a
 * count go out: never join codes, PINs or friends' names. Nothing while no
 * session runs. The backend (discord.rs) rate-limits and talks to Discord.
 */
export default function DiscordPresenceRunner() {
  const [on, setOn] = useState(loadDiscordPresence);
  const type = useAppStore((s) => s.session?.session_type ?? "None");
  const friends = useAppStore((s) => s.session?.peers.length ?? 0);
  const syncing = useAppStore((s) => !!s.session?.is_syncing);
  const game = useAppStore((s) => s.activeGame);
  // Tens of percent: a per-percent update would hit Discord's rate limit.
  const pct = useAppStore((s) => {
    const p = s.syncProgress;
    return p && p.bytes_total > 0 ? Math.min(100, Math.floor((p.bytes_sent / p.bytes_total) * 10) * 10) : null;
  });
  const startRef = useRef<number | null>(null);

  useEffect(() => {
    const update = () => setOn(loadDiscordPresence());
    window.addEventListener(DISCORD_PREF_EVENT, update);
    return () => window.removeEventListener(DISCORD_PREF_EVENT, update);
  }, []);

  const active = on && type !== "None";
  if (!active) startRef.current = null;
  else if (startRef.current === null) startRef.current = Math.floor(Date.now() / 1000);

  const label = getGameDef(game)?.label ?? "a game";
  let presence: cmd.DiscordPresence | null = null;
  if (active && type === "Host") {
    presence = { details: `Hosting ${label}`, state: friends > 0 ? `${plural(friends, "friend")} connected` : "Waiting for friends", start: startRef.current };
  } else if (active) {
    presence = { details: `Modding ${label} with friends`, state: pct !== null ? `Syncing mods: ${pct}%` : syncing ? "Syncing mods" : "Mods in sync", start: startRef.current };
  }
  const key = JSON.stringify(presence);

  useEffect(() => {
    cmd.setDiscordPresence(presence).catch(() => {});
    // `key` carries everything in `presence`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => () => void cmd.setDiscordPresence(null).catch(() => {}), []);
  return null;
}
