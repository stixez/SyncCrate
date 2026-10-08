import { useState, useMemo, useEffect, useRef, type ReactNode } from "react";
import { Plus, Check, ArrowRight, ArrowLeft, RefreshCw, Search, Radar, Monitor, Users, Link2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { BrandMark, GameIcon } from "./Sidebar";
import * as cmd from "../lib/commands";
import { loadDisplayName, saveDisplayName } from "../lib/prefs";
import { getGameDef } from "../lib/games";
import { isDemoMode } from "../lib/demoData";
import { parseJoinInput } from "../lib/utils";
import { toastError } from "../lib/toast";
import type { GameDefinition } from "../lib/types";
import { Button, GameArt, Input, LiveDot, cx } from "./ui";

const ONBOARDING_KEY = "synccrate-onboarding-complete";

// localStorage can throw (private mode, blocked storage): never let that
// break the first run.
export function isOnboardingComplete(): boolean {
  try {
    return localStorage.getItem(ONBOARDING_KEY) === "1";
  } catch {
    return false;
  }
}

export function markOnboardingComplete(): void {
  try {
    localStorage.setItem(ONBOARDING_KEY, "1");
  } catch {
    // Storage unavailable: onboarding shows again next time, which is harmless.
  }
}

const HOW_IT_WORKS: [string, string, string][] = [
  ["01", "Pick your games", "Tick the games you play. Ones we found are already ticked."],
  ["02", "One person hosts", "Whoever has the mods clicks Start Hosting and sends the join code."],
  ["03", "Everyone else joins", "Paste the code, click Compare & Sync, check the list, then Sync Now. Replaced files are kept, so you can undo."],
];

/** First run: most people install SyncCrate because a friend asked them to
 * join, or to share their own mods. The old "tick your games" screen left
 * both still hunting for the right button, so the first question is why
 * they're here; "Just look around" keeps the old screen. */
type Step = "why" | "host" | "join" | "join-game" | "browse";

const STEP_LABEL: Record<Step, string> = {
  why: "01 / Why you're here",
  host: "02 / Pick your game",
  join: "02 / Paste the invite",
  "join-game": "03 / Pick the game",
  browse: "01 / Pick your games",
};

export default function WelcomeScreen() {
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const installedGames = useAppStore((s) => s.installedGames);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const navigateToGlobal = useAppStore((s) => s.navigateToGlobal);

  const [step, setStep] = useState<Step>(() => {
    // ?demo&welcome=host (or join, join-game, browse): screenshots of later steps.
    const w = isDemoMode() ? new URLSearchParams(window.location.search).get("welcome") : null;
    return w && w in STEP_LABEL ? (w as Step) : "why";
  });
  const [busy, setBusy] = useState(false);

  // Friends saw a room of "Guest"s: nothing asked for a name before the
  // Dashboard's easy-to-miss field. Starts as the Windows account name.
  const [name, setName] = useState(loadDisplayName);
  useEffect(() => {
    if (name) return;
    cmd.defaultDisplayName().then((n) => setName((prev) => prev || n)).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const keepName = () => {
    if (name.trim()) saveDisplayName(name.trim());
  };
  const nameField = (
    <Input
      value={name}
      onChange={(e) => setName(e.target.value.replace(/[^\p{L}\p{N}\s_-]/gu, "").slice(0, 32))}
      label="// What friends call you"
      placeholder="e.g. Alex"
      aria-label="Your name"
      wrapperClassName="max-w-[320px]"
    />
  );

  const detected = useMemo(
    () => gameRegistry.filter((g) => installedGames.includes(g.id) && !myLibrary.includes(g.id)),
    [gameRegistry, installedGames, myLibrary],
  );

  // --- "Just look around": the original multi-select screen ---
  const [selected, setSelected] = useState<Set<string>>(() => new Set(detected.map((g) => g.id)));
  // The registry and installed games usually load after first render, so the
  // initializer above sees nothing. Pre-select each detected game once, and
  // never re-check one the user has unchecked.
  const autoSelected = useRef(new Set<string>(selected));
  useEffect(() => {
    const fresh = detected.filter((g) => !autoSelected.current.has(g.id));
    if (fresh.length === 0) return;
    fresh.forEach((g) => autoSelected.current.add(g.id));
    setSelected((prev) => new Set([...prev, ...fresh.map((g) => g.id)]));
  }, [detected]);

  const toggleGame = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleGetStarted = async () => {
    keepName();
    if (selected.size === 0) {
      markOnboardingComplete();
      navigateToGlobal("game-browser");
      return;
    }

    setBusy(true);
    const ids = Array.from(selected);
    const added: string[] = [];
    for (const id of ids) {
      try {
        if (!isDemoMode()) await cmd.addToLibrary(id);
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
      if (!isDemoMode()) await cmd.setActiveGame(added[0]).catch(() => {});
      navigateToGame(added[0]);
    } else {
      navigateToGlobal("game-browser");
    }
    setBusy(false);
  };

  const handleSkip = () => {
    keepName();
    markOnboardingComplete();
    navigateToGlobal("game-browser");
  };

  // --- Hosting / joining: one game ---
  const [picked, setPicked] = useState<string | null>(null);
  // Pre-pick the first detected game: most people host the one game they mod.
  useEffect(() => {
    if (!picked && detected.length > 0) setPicked(detected[0].id);
  }, [detected, picked]);

  const [joinText, setJoinText] = useState("");
  const [joinCode, setJoinCode] = useState<string | null>(null);
  const [joinError, setJoinError] = useState("");

  /** Add the game if needed, make it active and open its Dashboard with
   * `intent` for the Dashboard to act on. */
  const openGame = async (gameId: string, intent: { kind: "host" } | { kind: "join"; code: string }) => {
    keepName();
    setBusy(true);
    try {
      if (!isDemoMode()) {
        if (!myLibrary.includes(gameId)) await cmd.addToLibrary(gameId);
        await cmd.setActiveGame(gameId);
      } else {
        // The demo starts mid-session; show the screen a new user would see.
        useAppStore.setState({ session: null, syncPlan: null });
      }
      if (!myLibrary.includes(gameId)) setMyLibrary([...myLibrary, gameId]);
      useAppStore.getState().setActiveGame(gameId);
      useAppStore.getState().setFirstRunIntent(intent);
      markOnboardingComplete();
      navigateToGame(gameId);
    } catch (e) {
      toastError(`Couldn't open ${getGameDef(gameId)?.label ?? gameId}: ${e}`);
      setBusy(false);
    }
  };

  const submitJoin = () => {
    const parsed = parseJoinInput(joinText);
    if (!parsed) {
      setJoinError("That doesn't look like a join code. Codes start with SC-, links with synccrate.app/open.");
      return;
    }
    setJoinError("");
    keepName();
    // A link names the host's game; a bare code doesn't, so ask.
    if (parsed.game && getGameDef(parsed.game)) {
      openGame(parsed.game, { kind: "join", code: parsed.code });
    } else {
      setJoinCode(parsed.code);
      setStep("join-game");
    }
  };

  // --- Layout ---
  const hero = (
    <>
      <div className="flex items-center gap-3 mb-8">
        <BrandMark size={40} />
        <span className="font-display font-bold uppercase tracking-[0.08em] text-lg">SyncCrate</span>
      </div>
      <p className="hud-label mb-4 flex items-center gap-2">
        <LiveDot />
        <span><b>Setup</b> &nbsp;{STEP_LABEL[step]}</span>
      </p>
      <h1 className="display text-[3.4rem] leading-[0.92]">
        <span className="block">Same mods.</span>
        <span className="block text-neon">Every PC.</span>
        <span className="block text-transparent [-webkit-text-stroke:1.5px_rgb(var(--color-txt))]">No excuses.</span>
      </h1>
      <p className="text-txt-dim text-[15px] leading-relaxed mt-6 max-w-[44ch]">
        One friend hosts. Everyone else gets an exact copy of their mods, saves and settings, on the same Wi-Fi or over
        the internet with a join code.
      </p>
    </>
  );

  const back = (to: Step) => (
    <Button variant="ghost" size="lg" onClick={() => setStep(to)} disabled={busy} icon={<ArrowLeft size={15} />}>
      Back
    </Button>
  );

  let left: ReactNode;
  let right: ReactNode;

  if (step === "why") {
    left = hero;
    right = (
      <div className="corner-brackets min-w-0">
        <div className="panel px-6 pt-5 pb-6">
          <p className="hud-label mb-1">// Let's get you set up</p>
          <h2 className="font-display font-semibold uppercase tracking-[0.06em] text-[1.05rem] mb-5">Why did you install SyncCrate?</h2>
          <div className="flex flex-col gap-3">
            <ChoiceCard
              icon={<Monitor size={20} />}
              title="I'm hosting"
              text="My friends should get my mods."
              onClick={() => setStep("host")}
            />
            <ChoiceCard
              icon={<Users size={20} />}
              title="I'm joining a friend"
              text="They sent me a join code or an invite link."
              onClick={() => setStep("join")}
            />
          </div>
          <button
            onClick={() => setStep("browse")}
            className="mt-5 font-mono text-[11px] uppercase tracking-widest text-txt-muted hover:text-txt underline underline-offset-4 decoration-border hover:decoration-line-hi transition-colors"
          >
            Just look around
          </button>
        </div>
      </div>
    );
  } else if (step === "host" || step === "join-game") {
    const hosting = step === "host";
    const pickedLabel = picked ? getGameDef(picked)?.label ?? picked : null;
    left = (
      <>
        {hero}
        <div className="mt-8 flex flex-col gap-5">
          {hosting && nameField}
          <div className="flex items-center gap-3">
            {back(hosting ? "why" : "join")}
            <Button
              variant="primary"
              size="lg"
              disabled={!picked || busy || (!hosting && !joinCode)}
              onClick={() => {
                if (!picked) return;
                if (hosting) openGame(picked, { kind: "host" });
                else if (joinCode) openGame(picked, { kind: "join", code: joinCode });
              }}
              icon={busy ? <RefreshCw size={16} className="animate-spin" /> : undefined}
            >
              {busy ? "Opening..." : hosting ? "Continue" : "Join"}
              {!busy && <ArrowRight size={16} />}
            </Button>
          </div>
          <p className="font-mono text-[11px] uppercase tracking-widest text-txt-muted h-4 truncate">
            {pickedLabel ? <><span className="text-neon">{pickedLabel}</span> {hosting ? "selected" : "is the game you'll join"}</> : "Pick a game on the right"}
          </p>
        </div>
      </>
    );
    right = (
      <GamePicker
        label={hosting ? "// Hosting" : "// Joining"}
        title={hosting ? "Which game are you hosting?" : "Which game is your friend playing?"}
        isSelected={(id) => picked === id}
        onPick={setPicked}
        radio
      />
    );
  } else if (step === "join") {
    left = hero;
    right = (
      <div className="corner-brackets min-w-0">
        <div className="panel px-6 pt-5 pb-6">
          <p className="hud-label mb-1">// Joining</p>
          <h2 className="font-display font-semibold uppercase tracking-[0.06em] text-[1.05rem] mb-1">Paste your friend's join code or invite link</h2>
          <p className="text-[13px] text-txt-dim mb-4">
            Your friend sees it after clicking Start Hosting. The code starts with SC-.
          </p>
          <div className="flex items-stretch gap-2">
            <Input
              mono
              autoFocus
              value={joinText}
              onChange={(e) => {
                setJoinText(e.target.value);
                setJoinError("");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && joinText.trim() && !busy) submitJoin();
              }}
              placeholder="SC-XXXX-XXXX-… or synccrate.app/open/…"
              aria-label="Join code or invite link"
              aria-invalid={!!joinError}
              aria-describedby={joinError ? "join-error" : undefined}
              icon={<Link2 size={14} />}
              wrapperClassName="flex-1 min-w-0"
              className="h-11! text-[14px]"
            />
          </div>
          <p id="join-error" role={joinError ? "alert" : undefined} className="text-xs text-status-red mt-2 min-h-4">
            {joinError}
          </p>
          <div className="mt-3">{nameField}</div>
          <div className="flex items-center gap-3 mt-6">
            {back("why")}
            <Button
              variant="primary"
              size="lg"
              onClick={submitJoin}
              disabled={!joinText.trim() || busy}
              icon={busy ? <RefreshCw size={16} className="animate-spin" /> : undefined}
            >
              {busy ? "Opening..." : "Join"}
              {!busy && <ArrowRight size={16} />}
            </Button>
          </div>
        </div>
      </div>
    );
  } else {
    left = (
      <>
        {hero}
        <ol className="grid grid-cols-3 gap-3 mt-6 max-w-[560px]" aria-label="How SyncCrate works">
          {HOW_IT_WORKS.map(([n, title, text]) => (
            <li key={n} className="border border-border bg-bg-2 px-3 py-2.5">
              <p className="hud-label"><b>{n}</b> &nbsp;{title}</p>
              <p className="text-[12px] text-txt-dim leading-snug mt-1">{text}</p>
            </li>
          ))}
        </ol>
        <div className="mt-8">{nameField}</div>
        <div className="flex items-center gap-3 mt-5">
          {back("why")}
          <Button
            variant="primary"
            size="lg"
            onClick={handleGetStarted}
            disabled={busy}
            icon={busy ? <RefreshCw size={16} className="animate-spin" /> : selected.size > 0 ? undefined : <Plus size={16} />}
          >
            {busy ? (
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
        <p className="font-mono text-[11px] uppercase tracking-widest text-txt-muted mt-4 h-4">
          {selected.size > 0 && (
            <>
              <span className="text-neon tabular">{String(selected.size).padStart(2, "0")}</span> game{selected.size !== 1 ? "s" : ""} selected
            </>
          )}
        </p>
      </>
    );
    right = (
      <GamePicker label="// Your games" title="Choose what to sync" isSelected={(id) => selected.has(id)} onPick={toggleGame} />
    );
  }

  return (
    <div className="relative h-[calc(100%+3rem)] min-h-[560px] -mx-6 -my-6 px-10 py-10 hud-grid overflow-hidden">
      <div className="relative z-1 h-full grid grid-cols-[minmax(0,5fr)_minmax(0,6fr)] gap-10 items-center max-w-6xl mx-auto">
        <div className="min-w-0">{left}</div>
        {right}
      </div>
    </div>
  );
}

function ChoiceCard({ icon, title, text, onClick }: { icon: ReactNode; title: string; text: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="group flex items-center gap-4 text-left border border-border bg-bg px-4 py-4 hover:border-neon/60 hover:bg-neon/5 transition-colors"
    >
      <span className="w-11 h-11 shrink-0 grid place-items-center border border-border bg-bg-card text-neon group-hover:border-neon/60">
        {icon}
      </span>
      <span className="flex-1 min-w-0">
        <span className="block font-display font-semibold uppercase tracking-[0.06em] text-[1rem] text-txt">{title}</span>
        <span className="block text-[13px] text-txt-dim mt-0.5">{text}</span>
      </span>
      <ArrowRight size={18} className="shrink-0 text-txt-muted group-hover:text-neon transition-colors" />
    </button>
  );
}

/** Detected games first (with art), then every other supported game with a
 * filter. `radio`: pick exactly one (hosting/joining) instead of ticking many. */
function GamePicker({
  label,
  title,
  isSelected,
  onPick,
  radio = false,
}: {
  label: string;
  title: string;
  isSelected: (id: string) => boolean;
  onPick: (id: string) => void;
  radio?: boolean;
}) {
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const gamePaths = useAppStore((s) => s.gamePaths);
  const installedGames = useAppStore((s) => s.installedGames);
  const myLibrary = useAppStore((s) => s.myLibrary);

  // Picking one game (hosting/joining) may pick one already in the library;
  // ticking games to add only lists the ones not added yet.
  const listed = (id: string) => radio || !myLibrary.includes(id);
  const detected = useMemo(
    () => gameRegistry.filter((g) => installedGames.includes(g.id) && listed(g.id)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [gameRegistry, installedGames, myLibrary, radio],
  );
  const otherGames = useMemo(
    () => gameRegistry.filter((g) => !installedGames.includes(g.id) && listed(g.id)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [gameRegistry, installedGames, myLibrary, radio],
  );

  // With ~100 supported games the "other" list needs a filter to stay usable.
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const visibleOthers = q
    ? otherGames.filter((g) => g.label.toLowerCase().includes(q) || g.family.toLowerCase().includes(q))
    : otherGames;

  const renderGame = (game: GameDefinition, isDetected: boolean) => {
    const on = isSelected(game.id);
    return (
      <button
        key={game.id}
        onClick={() => onPick(game.id)}
        role={radio ? "radio" : undefined}
        aria-checked={radio ? on : undefined}
        aria-pressed={radio ? undefined : on}
        className={cx(
          "group relative flex items-center gap-3 pl-3 pr-3 py-2.5 border text-sm transition-colors text-left min-w-0",
          on ? "bg-neon/8 border-neon/60 text-txt" : "bg-bg border-border text-txt-dim hover:border-line-hi hover:text-txt",
        )}
      >
        <span className={cx("check pointer-events-none", on && "bg-neon! border-neon!")} aria-hidden="true">
          {on && <Check size={10} strokeWidth={3.5} className="text-neon-ink" />}
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
        {/* Folder exists but no install evidence (often left over from an uninstall) */}
        {!isDetected && gamePaths[game.id] && (
          <span className="tag text-txt-muted shrink-0" title="Its mods/saves folder exists, but the game doesn't look installed.">
            Folder found
          </span>
        )}
      </button>
    );
  };

  return (
    <div className="corner-brackets min-w-0">
      <div className="panel flex flex-col max-h-[min(600px,calc(var(--app-h)-11rem))]">
        <div className="px-5 pt-4 pb-3 border-b border-border flex items-end justify-between gap-3">
          <div className="min-w-0">
            <p className="hud-label mb-1">{label}</p>
            <h2 className="font-display font-semibold uppercase tracking-[0.06em] text-[0.95rem]">{title}</h2>
          </div>
          {otherGames.length > 8 && (
            <Input
              size="sm"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter games..."
              aria-label="Filter games"
              icon={<Search size={13} />}
              wrapperClassName="w-44 shrink-0"
            />
          )}
        </div>

        <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4 space-y-5" role={radio ? "radiogroup" : undefined} aria-label={radio ? title : undefined}>
          {detected.length > 0 && (
            <div>
              <p className="hud-label mb-2.5 flex items-center gap-2">
                <Radar size={12} className="text-neon" />
                <span><b>Detected</b> &nbsp;on your system</span>
              </p>
              <div className="grid grid-cols-2 gap-2">{detected.map((game) => renderGame(game, true))}</div>
            </div>
          )}

          {otherGames.length > 0 && (
            <div>
              <p className="hud-label mb-2.5">{detected.length > 0 ? "Other supported games" : "Supported games"}</p>
              <div className="grid grid-cols-2 gap-2">{visibleOthers.map((game) => renderGame(game, false))}</div>
              {visibleOthers.length === 0 && <p className="font-mono text-[11px] text-txt-muted">No games match "{filter}".</p>}
            </div>
          )}

          {/* No games at all (registry not loaded yet) */}
          {gameRegistry.length === 0 && (
            <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-widest text-txt-dim py-6">
              <RefreshCw size={13} className="animate-spin text-neon" />
              Loading games...
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
