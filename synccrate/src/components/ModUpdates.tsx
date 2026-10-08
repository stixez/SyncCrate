import { useEffect, useRef, useState } from "react";
import { ArrowUpCircle, ExternalLink, Loader2, RefreshCw } from "lucide-react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { Banner, Button } from "./ui";
import { useAppStore } from "../stores/useAppStore";
import * as cmd from "../lib/commands";
import { toastError, toastInfo } from "../lib/toast";
import type { ModMeta, UpdateReport } from "../lib/types";

export const UPDATE_SOURCE_LABELS: Record<string, string> = { modrinth: "Modrinth", thunderstore: "Thunderstore", smapi: "SMAPI", curseforge: "CurseForge" };

/** Games whose mod files are looked up on CurseForge (backend `curseforge::select_files`;
 * WoW addons by folder, for the clients CurseForge has builds for). */
const CURSEFORGE_GAMES = new Set(["sims4", "minecraft_java", "wow_retail", "wow_classic", "wow_classic_era", "wow_forever"]);

/** Whether any mod here has a source we can check (mirrors backend `mod_updates::checkable`). */
export function canCheckUpdates(metas: ModMeta[], gameId: string) {
  return (
    CURSEFORGE_GAMES.has(gameId) ||
    metas.some((m) => (["fabric", "quilt", "forge"].includes(m.source) && m.is_file) || m.source === "thunderstore" || m.source === "smapi")
  );
}

/** What the check sends, and to whom, for this game. */
function sendsWhat(gameId: string): string {
  const cf = "Sends file fingerprints (not names or files) to CurseForge through synccrate.app.";
  if (gameId === "sims4" || (gameId.startsWith("wow_") && CURSEFORGE_GAMES.has(gameId))) return cf;
  if (gameId === "minecraft_java") return `Asks Modrinth about your mods (their file hashes). ${cf}`;
  return "Asks Modrinth, Thunderstore and SMAPI about your mods (their ids and file hashes).";
}

/** "Check for updates" button + result summary for the Content page. Only
 * runs on click: it sends mod ids, hashes and fingerprints to Modrinth /
 * Thunderstore / SMAPI / CurseForge (through synccrate.app). */
export default function ModUpdates({ gameId, metas }: { gameId: string; metas: ModMeta[] }) {
  const report = useAppStore((s) => s.modUpdates[gameId]);
  const setModUpdates = useAppStore((s) => s.setModUpdates);
  const [checking, setChecking] = useState(false);
  const [expanded, setExpanded] = useState(false);
  // The page can close while a big folder is still being fingerprinted.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const nameOf = (key: string) => metas.find((m) => m.key === key)?.name ?? key.split("/").pop() ?? key;

  const check = async () => {
    setChecking(true);
    try {
      const r: UpdateReport = await cmd.checkModUpdates(gameId);
      setModUpdates(gameId, r);
      if (!mounted.current) {
        // CurseForge's part only lives while the page is open (their terms).
        useAppStore.getState().forgetCurseForge(gameId);
        cmd.forgetCurseforgeResults().catch(() => {});
        return;
      }
      if (r.updates.length === 0 && r.errors.length === 0) {
        const none = CURSEFORGE_GAMES.has(gameId) ? "None of these mods were found on CurseForge or another source we can check." : "None of these mods list an update source we can check.";
        toastInfo(r.checked > 0 ? `All ${r.checked} checked mods are up to date.` : none);
      }
    } catch (e) {
      toastError(`Couldn't check for updates: ${e}`);
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <Button size="sm" variant="secondary" onClick={check} disabled={checking} icon={checking ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}>
          {checking ? "Checking…" : "Check for updates"}
        </Button>
        <span className="text-[11px] text-txt-muted">{sendsWhat(gameId)}</span>
      </div>
      {report && report.updates.length > 0 && (
        <Banner
          tone="info"
          icon={<ArrowUpCircle size={14} />}
          title={`${report.updates.length} mod${report.updates.length !== 1 ? "s have" : " has"} an update`}
          actions={
            <Button size="sm" variant="ghost" onClick={() => setExpanded(!expanded)}>
              {expanded ? "Hide" : "Show"}
            </Button>
          }
        >
          <p>Download them from the mod's page, then sync as usual. Friends get them from you next time they sync.</p>
          {expanded && (
            <ul className="mt-2 space-y-1 max-h-56 overflow-y-auto">
              {report.updates.map((u) => (
                <li key={u.key} className="flex items-center gap-2 text-[12px]">
                  <span className="text-txt truncate flex-1" title={u.key}>
                    {nameOf(u.key)}{" "}
                    <span className="font-mono text-[10.5px] text-txt-muted">
                      {u.current ? `${u.current} → ` : ""}{u.latest}
                    </span>
                    {u.deprecated && <span className="text-amber ml-1.5">no longer maintained</span>}
                  </span>
                  <span className="font-mono text-[10px] uppercase text-txt-muted">{UPDATE_SOURCE_LABELS[u.source] ?? u.source}</span>
                  {u.changelog && (
                    <button className="font-mono text-[10.5px] text-accent-light hover:text-neon" onClick={() => openUrl(u.changelog!).catch(() => {})} aria-label={`What's new in ${nameOf(u.key)} ${u.latest}`}>
                      What's new
                    </button>
                  )}
                  {u.url && (
                    <button className="text-accent-light hover:text-neon" onClick={() => openUrl(u.url!).catch(() => {})} aria-label={`Open the update page for ${nameOf(u.key)}`}>
                      <ExternalLink size={12} />
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </Banner>
      )}
      {report && report.errors.length > 0 && (
        <p className="text-[11px] text-amber">Some sources didn't answer: {report.errors.join(" · ")}. Try again later.</p>
      )}
    </div>
  );
}
