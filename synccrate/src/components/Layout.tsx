import { ReactNode, useEffect } from "react";
import Sidebar from "./Sidebar";
import { Heart } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { getGameDef } from "../lib/games";
import DonateModal from "./DonateModal";
import { LiveDot } from "./ui";

const PAGE_LABELS: Record<string, string> = {
  dashboard: "Dashboard",
  content: "Content",
  profiles: "Profiles",
  backups: "Backups",
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

  useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  const isConnected = session && session.session_type !== "None";
  const isGlobal = page === "activity" || page === "settings" || page === "game-browser" || !selectedGame;
  const crumbGame = !isGlobal && selectedGame ? getGameDef(selectedGame)?.label ?? selectedGame : null;

  return (
    <div className="flex h-screen overflow-hidden bg-bg">
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
        <main className="flex-1 overflow-y-auto px-6 py-6">
          {children}
        </main>
      </div>
      {showDonate && <DonateModal />}
    </div>
  );
}
