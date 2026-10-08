import { useState, useEffect, useMemo, useRef } from "react";
import {
  LayoutDashboard,
  FolderOpen,
  Archive,
  Activity,
  Pin,
  BookOpen,
  Settings,
  Sun,
  Moon,
  Plus,
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
  Share2,
  Users,
} from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { resolveTheme, useMediaPreference } from "../lib/appearance";
import * as cmd from "../lib/commands";
import type { Page } from "../lib/types";
import { GameArt, LiveDot, cx } from "./ui";
import { toastAction, toastError } from "../lib/toast";
import { friendlyError } from "../lib/errors";
import { plural } from "../lib/utils";

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
  { page: "modpacks", label: "Modpacks", icon: Share2 },
];

/** Crate glyph on a clipped neon tile (the app icon, redrawn in the HUD style). */
export function BrandMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" className="shrink-0">
      <path d="M6 0H32V26L26 32H0V6Z" fill="rgb(var(--color-neon))" />
      {/* crate: frame, lid band and a diagonal brace */}
      <rect x="8" y="9" width="16" height="15" fill="none" stroke="rgb(var(--color-on-neon))" strokeWidth="2.4" />
      <path d="M8 13.5H24" stroke="rgb(var(--color-on-neon))" strokeWidth="2.4" />
      <path d="M9.5 22.5 22.5 14.5" stroke="rgb(var(--color-on-neon))" strokeWidth="2" />
    </svg>
  );
}

const PIN_KEY = "synccrate.pinnedGames";

export default function Sidebar() {
  const page = useAppStore((s) => s.page);
  const session = useAppStore((s) => s.session);
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const selectedGame = useAppStore((s) => s.selectedGame);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const navigateToGlobal = useAppStore((s) => s.navigateToGlobal);
  const isConnecting = useAppStore((s) => s.isConnecting);

  const [version, setVersion] = useState("...");
  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
  }, []);

  // The quick toggle flips whatever is showing now (so from "system" it pins the
  // opposite look); the full dark/light/system choice lives in Settings.
  useMediaPreference();
  const shownTheme = resolveTheme(theme);

  const isConnected = session && session.session_type !== "None";
  const activeGame = useAppStore((s) => s.activeGame);

  const [expandedGames, setExpandedGames] = useState<Set<string>>(new Set());
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);

  const hiddenGames = useAppStore((s) => s.hiddenGames);
  // Hidden games stay in the library (Settings → Games) but not here. The
  // selected game is always shown, so the sidebar never loses where you are.
  const shownGames = gameRegistry.filter((g) => myLibrary.includes(g.id) && (!hiddenGames.includes(g.id) || g.id === selectedGame));
  // Pinned games first (in pin order): with many games the one you play sat
  // wherever the registry put it. Per-PC convenience, so localStorage.
  const [pinned, setPinned] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(PIN_KEY) ?? "[]");
    } catch {
      return [];
    }
  });
  const togglePin = (id: string) =>
    setPinned((prev) => {
      const next = prev.includes(id) ? prev.filter((p) => p !== id) : [...prev, id];
      try { localStorage.setItem(PIN_KEY, JSON.stringify(next)); } catch { /* convenience only */ }
      return next;
    });
  const [gameFilter, setGameFilter] = useState("");
  const libraryGames = useMemo(() => {
    const q = gameFilter.trim().toLowerCase();
    const matching = q ? shownGames.filter((g) => g.label.toLowerCase().includes(q) || g.id === selectedGame) : shownGames;
    const pinnedFirst = pinned.map((id) => matching.find((g) => g.id === id)).filter((g): g is (typeof shownGames)[number] => !!g);
    return [...pinnedFirst, ...matching.filter((g) => !pinned.includes(g.id))];
  }, [shownGames, pinned, gameFilter, selectedGame]);
  const hiddenCount = gameRegistry.filter((g) => myLibrary.includes(g.id) && hiddenGames.includes(g.id) && g.id !== selectedGame).length;

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

  // One click on a hover "X" removed a game with no way back; offer Undo.
  const handleRemoveGame = async (gameId: string, label: string) => {
    try {
      await cmd.removeFromLibrary(gameId);
      setMyLibrary(await cmd.getUserLibrary());
      if (selectedGame === gameId) {
        navigateToGlobal("game-browser");
      }
      toastAction(`Removed ${label} from your library`, "Undo", () => {
        cmd.addToLibrary(gameId)
          .then(() => cmd.getUserLibrary())
          .then(setMyLibrary)
          .catch((e) => toastError(`Couldn't add ${label} back: ${friendlyError(e)}`));
      });
    } catch (e) {
      toastError(`Couldn't remove ${label}: ${friendlyError(e)}`);
    }
  };

  const globalLink = (p: Page, label: string, Icon: typeof Activity) => {
    const active = page === p && !selectedGame;
    return (
      <button
        onClick={() => navigateToGlobal(p)}
        className={cx(
          "row-h relative w-full flex items-center gap-3 px-4 h-9 font-display font-semibold text-[12px] uppercase tracking-widest transition-colors",
          active ? "text-txt bg-bg-card" : "text-txt-dim hover:text-txt hover:bg-bg-card/60",
        )}
      >
        {active && <span className="absolute left-0 top-0 bottom-0 w-[2px] bg-neon" />}
        <Icon size={15} className={active ? "text-neon" : ""} />
        {label}
      </button>
    );
  };

  const statusLabel = isConnected ? (session.session_type === "Host" ? "Hosting" : "Connected") : isConnecting ? "Connecting" : "Offline";
  const peerCount = isConnected ? session.peers.length : 0;

  return (
    <aside className="w-[224px] h-app bg-bg-2 border-r border-border flex flex-col shrink-0">
      <div className="h-14 px-4 flex items-center gap-2.5 border-b border-border shrink-0">
        <BrandMark />
        <span className="font-display font-bold text-[1.05rem] uppercase tracking-[0.08em] leading-none">SyncCrate</span>
      </div>

      <div className="flex-1 overflow-y-auto py-3">
        <div className="px-4 pb-2 flex items-center justify-between">
          <span className="hud-label"><b>//</b> Library</span>
          <span className="font-mono text-[10px] text-txt-muted tabular">{String(shownGames.length).padStart(2, "0")}</span>
        </div>
        {(shownGames.length > 8 || gameFilter) && (
          <div className="px-3 pb-2">
            <input
              value={gameFilter}
              onChange={(e) => setGameFilter(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setGameFilter("")}
              placeholder="Find a game..."
              aria-label="Find a game in your library"
              className="input input-sm w-full"
            />
          </div>
        )}

        {libraryGames.length === 0 && gameFilter.trim() ? (
          <p className="px-4 py-1 text-xs text-txt-muted">No game in your library matches.</p>
        ) : libraryGames.length === 0 ? (
          <div className="mx-4 my-1 border border-dashed border-line-hi px-3 py-3">
            <p className="text-xs text-txt-dim">No games added yet.</p>
            <button
              onClick={() => navigateToGlobal("game-browser")}
              className="text-xs text-neon hover:underline mt-1"
            >
              Add your first game
            </button>
          </div>
        ) : (
          libraryGames.map((game) => {
            const isSelected = selectedGame === game.id;
            const isExpanded = expandedGames.has(game.id);
            const color = game.primary_color || undefined;

            return (
              <div key={game.id} className={cx("relative", isSelected && "bg-bg-card")}>
                {isSelected && <span className="absolute left-0 top-0 bottom-0 w-[2px] bg-neon" />}
                <div className="flex items-center group">
                  <button
                    onClick={() => {
                      if (isSelected) {
                        toggleExpand(game.id);
                      } else {
                        navigateToGame(game.id);
                      }
                    }}
                    aria-expanded={isSelected ? isExpanded : undefined}
                    className={cx(
                      "row-y flex-1 min-w-0 flex items-center gap-3 pl-4 pr-1 py-2 text-left transition-colors",
                      isSelected ? "text-txt" : "text-txt-dim hover:text-txt hover:bg-bg-card/60",
                    )}
                  >
                    <span
                      className={cx(
                        "cut relative overflow-hidden w-8 h-8 shrink-0 grid place-items-center border transition-colors",
                        isSelected ? "bg-bg" : "bg-bg-card group-hover:bg-bg",
                      )}
                      style={{
                        ["--cut" as string]: "6px",
                        color,
                        borderColor: isSelected && color ? color : "rgb(var(--color-border))",
                      }}
                    >
                      <GameArt
                        gameId={game.id}
                        kind="cover"
                        className="absolute inset-0"
                        imgClassName={cx("object-[50%_22%]", !isSelected && "saturate-50 group-hover:saturate-100")}
                        fallback={<GameIcon iconName={game.icon} size={16} />}
                      />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] font-medium leading-tight">{game.label}</span>
                      {isSelected && isConnected && (
                        <span className="block font-mono text-[10px] text-neon uppercase tracking-widest mt-0.5">{statusLabel}</span>
                      )}
                    </span>
                  </button>
                  <button
                    onClick={() => togglePin(game.id)}
                    className={cx(
                      "p-1 text-txt-muted hover:text-neon transition-opacity",
                      pinned.includes(game.id) ? "opacity-100 text-neon/70" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
                    )}
                    title={pinned.includes(game.id) ? `Unpin ${game.label}` : `Pin ${game.label} to the top`}
                    aria-label={pinned.includes(game.id) ? `Unpin ${game.label}` : `Pin ${game.label} to the top`}
                    aria-pressed={pinned.includes(game.id)}
                  >
                    <Pin size={12} className={pinned.includes(game.id) ? "fill-current" : undefined} />
                  </button>
                  {!(isConnected && game.id === activeGame) && <button
                    onClick={() => handleRemoveGame(game.id, game.label)}
                    className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 p-1 mr-2 text-txt-muted hover:text-status-red transition-opacity"
                    title={`Remove ${game.label} from your library (your files stay)`}
                    aria-label={`Remove ${game.label} from your library (your files stay)`}
                  >
                    <X size={12} />
                  </button>}
                </div>

                {isSelected && isExpanded && (
                  <div className="pb-2 pl-[27px]">
                    <div className="border-l border-border">
                      {gameSubPages.map(({ page: p, label, icon: Icon }) => {
                        const active = page === p && selectedGame === game.id;
                        return (
                          <button
                            key={p}
                            onClick={() => navigateToGame(game.id, p)}
                            aria-current={active ? "page" : undefined}
                            className={cx(
                              "row-h-sm relative w-full flex items-center gap-2.5 pl-4 pr-3 h-8 font-display font-semibold text-[11.5px] uppercase tracking-widest transition-colors",
                              active ? "text-neon" : "text-txt-muted hover:text-txt",
                            )}
                          >
                            {active && <span className="absolute -left-px top-1.5 bottom-1.5 w-[2px] bg-neon" />}
                            <Icon size={13} />
                            {label}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            );
          })
        )}

        {hiddenCount > 0 && (
          <button
            onClick={() => navigateToGlobal("settings")}
            className="mx-4 mt-1 font-mono text-[10px] uppercase tracking-[0.08em] text-txt-muted hover:text-txt"
            title="Show them again in Settings → Games"
          >
            {plural(hiddenCount, "hidden game")}
          </button>
        )}

        <button
          onClick={() => navigateToGlobal("game-browser")}
          className={cx(
            "w-full flex items-center gap-3 pl-4 pr-3 py-2 mt-1 text-left transition-colors group",
            page === "game-browser" && !selectedGame ? "text-neon" : "text-txt-muted hover:text-neon",
          )}
        >
          <span className="w-8 h-8 shrink-0 grid place-items-center border border-dashed border-line-hi group-hover:border-neon transition-colors">
            <Plus size={14} />
          </span>
          <span className="font-display font-semibold text-[12px] uppercase tracking-widest">Add Game</span>
        </button>

        <div className="mt-4 pt-3 border-t border-border">
          <p className="hud-label px-4 pb-2"><b>//</b> System</p>
          {globalLink("crews", "Crews", Users)}
          {globalLink("activity", "Activity Log", Activity)}
          {globalLink("guide", "Guide", BookOpen)}
          {globalLink("settings", "Settings", Settings)}
        </div>
      </div>

      <div className="border-t border-border p-3 shrink-0">
        <div className="bg-bg px-3 py-2.5 border border-border">
          <div className="flex items-center gap-2">
            <LiveDot tone={isConnected ? "neon" : isConnecting ? "amber" : "idle"} />
            <span
              className={cx(
                "font-display font-bold text-[12px] uppercase tracking-[0.12em]",
                isConnected ? "text-neon" : isConnecting ? "text-amber" : "text-txt-dim",
              )}
            >
              {statusLabel}
            </span>
            {isConnected && (
              <span className="ml-auto font-mono text-[10px] text-txt-dim tabular">
                {plural(peerCount, "friend")}
              </span>
            )}
          </div>
          {isConnected && session.name && (
            <p className="font-mono text-[10px] text-txt-muted mt-1 truncate uppercase tracking-[0.08em]">{session.name}</p>
          )}
        </div>
        <div className="flex items-center justify-between mt-2 px-1">
          <span className="font-mono text-[10px] text-txt-muted tracking-[0.08em]">v{version}</span>
          <button
            onClick={() => setTheme(shownTheme === "dark" ? "light" : "dark")}
            className="p-1.5 text-txt-muted hover:text-txt hover:bg-bg-card transition-colors"
            title={shownTheme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
            aria-label={shownTheme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
          >
            {shownTheme === "dark" ? <Sun size={14} /> : <Moon size={14} />}
          </button>
        </div>
      </div>
    </aside>
  );
}
