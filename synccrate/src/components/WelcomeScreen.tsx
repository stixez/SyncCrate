import { useState, useMemo, useEffect, useRef } from "react";
import { Plus, Check, ArrowRight, RefreshCw, Search, Radar } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { BrandMark, GameIcon } from "./Sidebar";
import * as cmd from "../lib/commands";
import type { GameDefinition } from "../lib/types";
import { Button, GameArt, Input, LiveDot, cx } from "./ui";

const ONBOARDING_KEY = "synccrate-onboarding-complete";

export function isOnboardingComplete(): boolean {
  return localStorage.getItem(ONBOARDING_KEY) === "1";
}

export function markOnboardingComplete(): void {
  localStorage.setItem(ONBOARDING_KEY, "1");
}

export default function WelcomeScreen() {
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const gamePaths = useAppStore((s) => s.gamePaths);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const navigateToGlobal = useAppStore((s) => s.navigateToGlobal);

  const [selected, setSelected] = useState<Set<string>>(() => {
    // Pre-select all detected games
    const detected = new Set<string>();
    for (const g of gameRegistry) {
      if (gamePaths[g.id] && !myLibrary.includes(g.id)) {
        detected.add(g.id);
      }
    }
    return detected;
  });
  const [adding, setAdding] = useState(false);

  const detected = useMemo(
    () => gameRegistry.filter((g) => gamePaths[g.id] && !myLibrary.includes(g.id)),
    [gameRegistry, gamePaths, myLibrary],
  );

  // The registry and detected paths usually load after first render, so the
  // initializer above sees nothing. Pre-select each detected game once, and
  // never re-check one the user has unchecked.
  const autoSelected = useRef(new Set<string>(selected));
  useEffect(() => {
    const fresh = detected.filter((g) => !autoSelected.current.has(g.id));
    if (fresh.length === 0) return;
    fresh.forEach((g) => autoSelected.current.add(g.id));
    setSelected((prev) => new Set([...prev, ...fresh.map((g) => g.id)]));
  }, [detected]);

  const otherGames = useMemo(
    () => gameRegistry.filter((g) => !gamePaths[g.id] && !myLibrary.includes(g.id)),
    [gameRegistry, gamePaths, myLibrary],
  );

  const toggleGame = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleGetStarted = async () => {
    if (selected.size === 0) {
      markOnboardingComplete();
      navigateToGlobal("game-browser");
      return;
    }

    setAdding(true);
    const ids = Array.from(selected);
    const added: string[] = [];
    for (const id of ids) {
      try {
        await cmd.addToLibrary(id);
        added.push(id);
      } catch {
        // Skip failed games, continue with the rest
      }
    }
    if (added.length > 0) {
      setMyLibrary([...myLibrary, ...added]);
    }
    markOnboardingComplete();
    if (added.length > 0) {
      await cmd.setActiveGame(added[0]).catch(() => {});
      navigateToGame(added[0]);
    } else {
      navigateToGlobal("game-browser");
    }
    setAdding(false);
  };

  const handleSkip = () => {
    markOnboardingComplete();
    navigateToGlobal("game-browser");
  };

  // With ~100 supported games the "other" list needs a filter to stay usable.
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const visibleOthers = q
    ? otherGames.filter((g) => g.label.toLowerCase().includes(q) || g.family.toLowerCase().includes(q))
    : otherGames;

  const renderGame = (game: GameDefinition, isDetected: boolean) => {
    const isSelected = selected.has(game.id);
    return (
      <button
        key={game.id}
        onClick={() => toggleGame(game.id)}
        aria-pressed={isSelected}
        className={cx(
          "group relative flex items-center gap-3 pl-3 pr-3 py-2.5 border text-sm transition-colors text-left min-w-0",
          isSelected
            ? "bg-neon/[0.08] border-neon/60 text-txt"
            : "bg-bg border-border text-txt-dim hover:border-line-hi hover:text-txt",
        )}
      >
        <span className={cx("check pointer-events-none", isSelected && "!bg-neon !border-neon")} aria-hidden="true">
          {isSelected && <Check size={10} strokeWidth={3.5} className="text-neon-ink" />}
        </span>
        {/* Art only for detected games: the full list would download ~100 images on first run */}
        {isDetected ? (
          <span className="relative overflow-hidden w-6 h-6 shrink-0 grid place-items-center border border-border bg-bg-card">
            <GameArt
              gameId={game.id}
              kind="cover"
              className="absolute inset-0"
              imgClassName="object-[50%_22%]"
              fallback={<GameIcon iconName={game.icon} size={14} className={game.color} />}
            />
          </span>
        ) : (
          <GameIcon iconName={game.icon} size={15} className={cx("shrink-0", game.color)} />
        )}
        <span className="truncate flex-1">{game.label}</span>
        {isDetected && <span className="tag text-status-green shrink-0">Found</span>}
      </button>
    );
  };

  return (
    <div className="relative h-[calc(100%+3rem)] min-h-[560px] -mx-6 -my-6 px-10 py-10 hud-grid overflow-hidden">
      <div className="relative z-[1] h-full grid grid-cols-[minmax(0,5fr)_minmax(0,6fr)] gap-10 items-center max-w-6xl mx-auto">
        {/* Hero */}
        <div className="min-w-0">
          <div className="flex items-center gap-3 mb-8">
            <BrandMark size={40} />
            <span className="font-display font-bold uppercase tracking-[0.08em] text-lg">SyncCrate</span>
          </div>
          <p className="hud-label mb-4 flex items-center gap-2">
            <LiveDot />
            <span><b>Setup</b> &nbsp;01 / Pick your games</span>
          </p>
          <h1 className="display text-[3.4rem] leading-[0.92]">
            <span className="block">Same mods.</span>
            <span className="block text-neon">Every PC.</span>
            <span className="block text-transparent [-webkit-text-stroke:1.5px_rgb(var(--color-txt))]">No excuses.</span>
          </h1>
          <p className="text-txt-dim text-[15px] leading-relaxed mt-6 max-w-[44ch]">
            Welcome to SyncCrate. Sync your game mods, saves, and settings with friends — on your LAN or over the internet
            with a join code.
          </p>

          <div className="flex items-center gap-3 mt-8">
            <Button
              variant="primary"
              size="lg"
              onClick={handleGetStarted}
              disabled={adding}
              icon={adding ? <RefreshCw size={16} className="animate-spin" /> : selected.size > 0 ? undefined : <Plus size={16} />}
            >
              {adding ? (
                "Adding..."
              ) : selected.size > 0 ? (
                <>
                  Get Started
                  <ArrowRight size={16} />
                </>
              ) : (
                "Browse Games"
              )}
            </Button>
            {selected.size > 0 && (
              <Button variant="ghost" size="lg" onClick={handleSkip}>
                Skip
              </Button>
            )}
          </div>
          <p className="font-mono text-[11px] uppercase tracking-[0.1em] text-txt-muted mt-4 h-4">
            {selected.size > 0 && (
              <>
                <span className="text-neon tabular">{String(selected.size).padStart(2, "0")}</span> game{selected.size !== 1 ? "s" : ""} selected
              </>
            )}
          </p>
        </div>

        {/* Game picker */}
        <div className="corner-brackets min-w-0">
          <div className="panel flex flex-col max-h-[min(600px,calc(var(--app-h)-11rem))]">
            <div className="px-5 pt-4 pb-3 border-b border-border flex items-end justify-between gap-3">
              <div>
                <p className="hud-label mb-1">// Your games</p>
                <h2 className="font-display font-semibold uppercase tracking-[0.06em] text-[0.95rem]">Choose what to sync</h2>
              </div>
              {otherGames.length > 8 && (
                <Input
                  size="sm"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter games..."
                  aria-label="Filter games"
                  icon={<Search size={13} />}
                  wrapperClassName="w-44"
                />
              )}
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4 space-y-5">
              {/* Detected games */}
              {detected.length > 0 && (
                <div>
                  <p className="hud-label mb-2.5 flex items-center gap-2">
                    <Radar size={12} className="text-neon" />
                    <span><b>Detected</b> &nbsp;on your system</span>
                  </p>
                  <div className="grid grid-cols-2 gap-2">
                    {detected.map((game) => renderGame(game, true))}
                  </div>
                </div>
              )}

              {/* Other games */}
              {otherGames.length > 0 && (
                <div>
                  <p className="hud-label mb-2.5">
                    {detected.length > 0 ? "Other supported games" : "Supported games"}
                  </p>
                  <div className="grid grid-cols-2 gap-2">
                    {visibleOthers.map((game) => renderGame(game, false))}
                  </div>
                  {visibleOthers.length === 0 && (
                    <p className="font-mono text-[11px] text-txt-muted">No games match "{filter}".</p>
                  )}
                </div>
              )}

              {/* No games at all (registry not loaded yet) */}
              {gameRegistry.length === 0 && (
                <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.1em] text-txt-dim py-6">
                  <RefreshCw size={13} className="animate-spin text-neon" />
                  Loading game registry...
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
