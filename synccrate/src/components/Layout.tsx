import { ReactNode, useEffect } from "react";
import Sidebar from "./Sidebar";
import { Heart } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { gamePrimaryColor, getGameDef } from "../lib/games";
import { applyAccent, applyAppearanceRoot, applyThemeClass, useMediaPreference } from "../lib/appearance";
import DonateModal from "./DonateModal";
import { GameArt, LiveDot } from "./ui";

const PAGE_LABELS: Record<string, string> = {
  dashboard: "Dashboard",
  content: "Content",
  profiles: "Profiles",
  backups: "Backups",
  modpacks: "Modpacks",
  crews: "Crews",
  activity: "Activity Log",
  settings: "Settings",
  "game-browser": "Game Browser",
};

export default function Layout({ children }: { children: ReactNode }) {
  const showDonate = useAppStore((s) => s.showDonate);
  const setShowDonate = useAppStore((s) => s.setShowDonate);
  const session = useAppStore((s) => s.session);
  const theme = useAppStore((s) => s.theme);
  const page = useAppStore((s) => s.page);
  const selectedGame = useAppStore((s) => s.selectedGame);

  const appearance = useAppStore((s) => s.appearance);
  const mediaTick = useMediaPreference();

  // mediaTick re-runs these when the OS scheme / reduced-motion setting flips,
  // so "system" theme and auto effects follow it live.
  useEffect(() => {
    applyThemeClass(theme);
  }, [theme, mediaTick]);

  useEffect(() => {
    applyAppearanceRoot(appearance);
  }, [appearance, mediaTick]);

  // The user's accent by default; per-game recoloring is opt-in.
  const accentSource = appearance.matchGame && selectedGame ? gamePrimaryColor(selectedGame) : appearance.accent;
  useEffect(() => {
    applyAccent(accentSource);
  }, [accentSource]);

  const isConnected = session && session.session_type !== "None";
  const isGlobal = page === "activity" || page === "settings" || page === "game-browser" || page === "crews" || !selectedGame;
  const crumbGame = !isGlobal && selectedGame ? getGameDef(selectedGame)?.label ?? selectedGame : null;

  return (
    <div className="flex h-app overflow-hidden bg-bg">
      <Sidebar />
      <div className="flex-1 min-w-0 flex flex-col overflow-hidden">
        <header className="h-14 border-b border-border flex items-center justify-between gap-4 px-6 shrink-0 bg-bg">
          <nav aria-label="Breadcrumb" className="font-mono text-[11px] uppercase tracking-[0.12em] text-txt-muted truncate">
            <span className="text-neon">//</span>{" "}
            {crumbGame && (
              <>
                <span className="text-txt-dim">{crumbGame}</span>
                <span className="mx-2 text-line-hi">/</span>
              </>
            )}
            <span className="text-txt">{PAGE_LABELS[page] ?? page}</span>
          </nav>
          <div className="flex items-center gap-4 shrink-0">
            {isConnected && (
              <span className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.12em] text-txt-dim">
                <LiveDot />
                <span className="text-neon">{session.session_type === "Host" ? "Hosting" : "Connected"}</span>
                {session.peers.length > 0 && (
                  <span className="text-txt-muted">
                    / {session.peers.length} peer{session.peers.length > 1 ? "s" : ""}
                  </span>
                )}
              </span>
            )}
            <button
              onClick={() => setShowDonate(true)}
              className="btn btn-ghost btn-sm hover:!text-pink-400"
            >
              <Heart size={13} />
              Donate
            </button>
          </div>
        </header>
        <main className="relative flex-1 overflow-y-auto px-6 py-6">
          {/* Game pages get the game's Steam hero art as a dim, masked backdrop
              behind the header; it scrolls away with the content. */}
          {!isGlobal && selectedGame && (
            <GameArt
              key={selectedGame}
              gameId={selectedGame}
              kind="hero"
              className="absolute inset-x-0 top-0 h-[320px] pointer-events-none opacity-[0.42] [mask-image:linear-gradient(to_bottom,black_20%,transparent_100%)]"
              imgClassName="object-[50%_28%] saturate-[0.8]"
            >
              <div className="absolute inset-0 bg-gradient-to-r from-bg via-bg/55 to-bg/10" />
              <div className="fx-deco absolute inset-0 opacity-40 bg-[repeating-linear-gradient(0deg,transparent_0_2px,rgb(0_0_0/0.4)_2px_3px)]" />
            </GameArt>
          )}
          <div className="relative z-[1]">{children}</div>
        </main>
      </div>
      {showDonate && <DonateModal />}
    </div>
  );
}
