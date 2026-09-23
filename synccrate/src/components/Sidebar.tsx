import { useState, useEffect, useRef } from "react";
import {
  LayoutDashboard,
  FolderOpen,
  Archive,
  Activity,
  Settings,
  Wifi,
  WifiOff,
  Sun,
  Moon,
  Plus,
  ChevronRight,
  ChevronDown,
  Package,
  Gamepad2,
  Swords,
  Box,
  Crosshair,
  Wrench,
  Sprout,
  Axe,
  Pickaxe,
  Target,
  Castle,
  Rocket,
  Flag,
  Flame,
  FlameKindling,
  Zap,
  Shield,
  Factory,
  Sparkles,
  Layers,
  Truck,
  Cog,
  Building2,
  PawPrint,
  Biohazard,
  Waves,
  Skull,
  Trees,
  X,
} from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { gamePrimaryColor } from "../lib/games";
import { applyGameTheme } from "../lib/theme";
import * as cmd from "../lib/commands";
import type { Page } from "../lib/types";

const ICON_MAP: Record<string, typeof Gamepad2> = {
  "gamepad-2": Gamepad2,
  swords: Swords,
  box: Box,
  crosshair: Crosshair,
  wrench: Wrench,
  sprout: Sprout,
  axe: Axe,
  pickaxe: Pickaxe,
  target: Target,
  castle: Castle,
  rocket: Rocket,
  flag: Flag,
  flame: Flame,
  "flame-kindling": FlameKindling,
  zap: Zap,
  shield: Shield,
  factory: Factory,
  package: Package,
  sun: Sun,
  sparkles: Sparkles,
  layers: Layers,
  truck: Truck,
  cog: Cog,
  "building-2": Building2,
  "paw-print": PawPrint,
  biohazard: Biohazard,
  waves: Waves,
  skull: Skull,
  trees: Trees,
};

export function GameIcon({ iconName, size = 14, className = "" }: { iconName: string; size?: number; className?: string }) {
  const Icon = ICON_MAP[iconName] || Gamepad2;
  return <Icon size={size} className={className} />;
}

const gameSubPages: { page: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { page: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { page: "content", label: "Content", icon: Package },
  { page: "profiles", label: "Profiles", icon: FolderOpen },
  { page: "backups", label: "Backups", icon: Archive },
];

export default function Sidebar() {
  const page = useAppStore((s) => s.page);
  const setPage = useAppStore((s) => s.setPage);
  const session = useAppStore((s) => s.session);
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const selectedGame = useAppStore((s) => s.selectedGame);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const navigateToGlobal = useAppStore((s) => s.navigateToGlobal);

  const [version, setVersion] = useState("...");
  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
  }, []);

  // Apply dynamic accent color when selected game changes
  useEffect(() => {
    if (selectedGame) {
      applyGameTheme(gamePrimaryColor(selectedGame));
    } else {
      applyGameTheme(null);
    }
  }, [selectedGame]);

  const isConnected = session && session.session_type !== "None";

  const [expandedGames, setExpandedGames] = useState<Set<string>>(new Set());
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);

  const libraryGames = gameRegistry.filter((g) => myLibrary.includes(g.id));

  // Auto-expand the selected game
  useEffect(() => {
    if (selectedGame && !expandedGames.has(selectedGame)) {
      setExpandedGames((prev) => new Set(prev).add(selectedGame));
    }
  }, [selectedGame]);

  const toggleExpand = (gameId: string) => {
    setExpandedGames((prev) => {
      const next = new Set(prev);
      if (next.has(gameId)) {
        next.delete(gameId);
      } else {
        next.add(gameId);
      }
      return next;
    });
  };

  const handleRemoveGame = async (gameId: string) => {
    try {
      await cmd.removeFromLibrary(gameId);
      const updated = await cmd.getUserLibrary();
      setMyLibrary(updated);
      if (selectedGame === gameId) {
        navigateToGlobal("game-browser");
      }
    } catch (e) {
      console.error("Failed to remove game:", e);
    }
  };

  return (
    <aside className="w-[200px] h-screen bg-bg-card border-r border-border flex flex-col shrink-0">
      <div className="p-4 border-b border-border">
        <h1 className="text-lg font-bold text-accent-light tracking-tight">SyncCrate</h1>
        <p className="text-[10px] text-txt-dim mt-0.5">v{version}</p>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        <p className="px-4 py-1 text-[10px] font-semibold text-txt-dim uppercase tracking-wider">
          My Games
        </p>

        {libraryGames.length === 0 ? (
          <div className="px-4 py-3">
            <p className="text-xs text-txt-dim">No games added yet.</p>
            <button
              onClick={() => navigateToGlobal("game-browser")}
              className="text-xs text-accent-light hover:underline mt-1"
            >
              Add your first game
            </button>
          </div>
        ) : (
          libraryGames.map((game) => {
            const isSelected = selectedGame === game.id;
            const isExpanded = expandedGames.has(game.id);

            return (
              <div key={game.id}>
                <div className="flex items-center group">
                  <button
                    onClick={() => {
                      if (isSelected) {
                        toggleExpand(game.id);
                      } else {
                        navigateToGame(game.id);
                      }
                    }}
                    className={`flex-1 flex items-center gap-2 px-4 py-1.5 text-sm transition-colors ${
                      isSelected
                        ? "text-accent-light bg-bg-card-active"
                        : "text-txt-dim hover:text-txt hover:bg-bg-card-hover"
                    }`}
                  >
                    {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                    <GameIcon iconName={game.icon} size={14} className={isSelected ? "text-accent-light" : ""} />
                    <span className="truncate flex-1 text-left">{game.label}</span>
                  </button>
                  <button
                    onClick={() => handleRemoveGame(game.id)}
                    className="opacity-0 group-hover:opacity-100 p-1 mr-2 rounded text-txt-dim hover:text-status-red transition-all"
                    title={`Remove ${game.label}`}
                  >
                    <X size={12} />
                  </button>
                </div>

                {isExpanded && (
                  <div className="ml-4">
                    {gameSubPages.map(({ page: p, label, icon: Icon }) => (
                      <button
                        key={p}
                        onClick={() => navigateToGame(game.id, p)}
                        className={`w-full flex items-center gap-2 pl-6 pr-4 py-1.5 text-xs transition-colors ${
                          page === p && selectedGame === game.id
                            ? "text-accent-light bg-bg-card-active border-r-2 border-accent"
                            : "text-txt-dim hover:text-txt hover:bg-bg-card-hover"
                        }`}
                      >
                        <Icon size={13} />
                        {label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            );
          })
        )}

        <button
          onClick={() => navigateToGlobal("game-browser")}
          className="w-full flex items-center gap-2 px-4 py-1.5 text-xs text-txt-dim hover:text-accent-light hover:bg-bg-card-hover transition-colors mt-1"
        >
          <Plus size={13} />
          Add Game
        </button>

        <div className="border-t border-border mt-3 pt-2">
          <button
            onClick={() => navigateToGlobal("activity")}
            className={`w-full flex items-center gap-2.5 px-4 py-2 text-sm transition-colors ${
              page === "activity" && !selectedGame
                ? "bg-bg-card-active text-accent-light border-r-2 border-accent"
                : "text-txt-dim hover:text-txt hover:bg-bg-card-hover"
            }`}
          >
            <Activity size={16} />
            Activity Log
          </button>
          <button
            onClick={() => navigateToGlobal("settings")}
            className={`w-full flex items-center gap-2.5 px-4 py-2 text-sm transition-colors ${
              page === "settings" && !selectedGame
                ? "bg-bg-card-active text-accent-light border-r-2 border-accent"
                : "text-txt-dim hover:text-txt hover:bg-bg-card-hover"
            }`}
          >
            <Settings size={16} />
            Settings
          </button>
        </div>
      </div>

      <div className="p-4 border-t border-border space-y-2">
        <div className="flex items-center gap-2 text-xs">
          {isConnected ? (
            <>
              <Wifi size={14} className="text-status-green" />
              <span className="text-status-green">
                {session.session_type === "Host" ? "Hosting" : "Connected"}
                {session.peers.length > 0 && ` \u2022 ${session.peers.length} peer${session.peers.length > 1 ? "s" : ""}`}
              </span>
            </>
          ) : (
            <>
              <WifiOff size={14} className="text-txt-dim" />
              <span className="text-txt-dim">Not connected</span>
            </>
          )}
          <button
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            className="ml-auto p-1 rounded hover:bg-bg-card-hover text-txt-dim hover:text-txt transition-colors"
            title={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
          >
            {theme === "dark" ? <Sun size={14} /> : <Moon size={14} />}
          </button>
        </div>
      </div>
    </aside>
  );
}
