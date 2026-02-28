import { ReactNode, useEffect } from "react";
import Sidebar from "./Sidebar";
import { Heart } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import DonateModal from "./DonateModal";

export default function Layout({ children }: { children: ReactNode }) {
  const showDonate = useAppStore((s) => s.showDonate);
  const setShowDonate = useAppStore((s) => s.setShowDonate);
  const session = useAppStore((s) => s.session);
  const theme = useAppStore((s) => s.theme);

  useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  const isConnected = session && session.session_type !== "None";

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <header className="h-12 border-b border-border flex items-center justify-between px-4 shrink-0 bg-bg-card">
          <div />
          <div className="flex items-center gap-3">
            {isConnected && (
              <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-status-green/10 text-status-green text-xs font-medium">
                <span className="w-1.5 h-1.5 rounded-full bg-status-green animate-pulse" />
                {session.session_type === "Host" ? "Hosting" : "Connected"}
                {session.peers.length > 0 && ` • ${session.peers.length} peer${session.peers.length > 1 ? "s" : ""}`}
              </span>
            )}
            <button
              onClick={() => setShowDonate(true)}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-bg-card-hover hover:bg-bg-card-active text-txt-dim hover:text-pink-400 transition-colors text-xs"
            >
              <Heart size={14} />
              Donate
            </button>
          </div>
        </header>
        <main className="flex-1 overflow-y-auto p-6">
          {children}
        </main>
      </div>
      {showDonate && <DonateModal />}
    </div>
  );
}
