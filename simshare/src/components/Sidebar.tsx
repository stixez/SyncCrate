import { useState, useEffect } from "react";
import { LayoutDashboard, Package, Save, FolderOpen, Archive, Activity, Settings, Wifi, WifiOff } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import * as cmd from "../lib/commands";
import type { Page } from "../lib/types";

const navItems: { page: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { page: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { page: "mods", label: "Mods & CC", icon: Package },
  { page: "saves", label: "Saves", icon: Save },
  { page: "profiles", label: "Profiles", icon: FolderOpen },
  { page: "backups", label: "Backups", icon: Archive },
  { page: "activity", label: "Activity Log", icon: Activity },
  { page: "settings", label: "Settings", icon: Settings },
];

export default function Sidebar() {
  const page = useAppStore((s) => s.page);
  const setPage = useAppStore((s) => s.setPage);
  const session = useAppStore((s) => s.session);

  const [version, setVersion] = useState("...");
  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
  }, []);

  const isConnected = session && session.session_type !== "None";

  return (
    <aside className="w-[180px] h-screen bg-bg-card border-r border-border flex flex-col shrink-0">
      <div className="p-4 border-b border-border">
        <h1 className="text-lg font-bold text-accent-light tracking-tight">SimShare</h1>
        <p className="text-[10px] text-txt-dim mt-0.5">v{version}</p>
      </div>

      <nav className="flex-1 py-2">
        {navItems.map(({ page: p, label, icon: Icon }) => (
          <button
            key={p}
            onClick={() => setPage(p)}
            className={`w-full flex items-center gap-2.5 px-4 py-2 text-sm transition-colors ${
              page === p
                ? "bg-bg-card-active text-accent-light border-r-2 border-accent"
                : "text-txt-dim hover:text-txt hover:bg-bg-card-hover"
            }`}
          >
            <Icon size={16} />
            {label}
          </button>
        ))}
      </nav>

      <div className="p-4 border-t border-border">
        <div className="flex items-center gap-2 text-xs">
          {isConnected ? (
            <>
              <Wifi size={14} className="text-status-green" />
              <span className="text-status-green">
                {session.session_type === "Host" ? "Hosting" : "Connected"}
                {session.peers.length > 0 && ` • ${session.peers.length} peer${session.peers.length > 1 ? "s" : ""}`}
              </span>
            </>
          ) : (
            <>
              <WifiOff size={14} className="text-txt-dim" />
              <span className="text-txt-dim">Not connected</span>
            </>
          )}
        </div>
      </div>
    </aside>
  );
}
