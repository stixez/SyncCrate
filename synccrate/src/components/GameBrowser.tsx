import { useState, useMemo, useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { Search, Plus, Check, ArrowRight, Radar, LayoutGrid, List, X, ArrowUpDown, Folder } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { GameIcon } from "./Sidebar";
import * as cmd from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import { open } from "@tauri-apps/plugin-shell";

const REQUEST_GAME_URL = "https://github.com/stixez/synccrate/issues/new?template=game_request.md";
import { genreLabel } from "../lib/games";
import { isDemoMode } from "../lib/demoData";
import type { GameDefinition } from "../lib/types";
import { Button, EmptyState, GameArt, Input, SectionHeader, cx } from "./ui";

type Status = "all" | "library" | "detected" | "available";
type Sort = "name" | "series";
type View = "grid" | "list";

// View and sort are remembered per user; filters reset each visit on purpose
// (a stale genre filter would make games look "missing").
const VIEW_KEY = "synccrate-browser-view";
const SORT_KEY = "synccrate-browser-sort";

function readPref<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable — ignore
  }
}

export default function GameBrowser() {
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);
  const gamePaths = useAppStore((s) => s.gamePaths);
  const installedGames = useAppStore((s) => s.installedGames);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const addLog = useLogStore((s) => s.addLog);

  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<Status>("all");
  const [genres, setGenres] = useState<Set<string>>(new Set());
  const [sort, setSortState] = useState<Sort>(() => readPref(SORT_KEY, ["name", "series"] as const, "name"));
  const [view, setViewState] = useState<View>(() =>
    // `?demo&view=list` for screenshots (headless runs start with empty storage).
    isDemoMode() && new URLSearchParams(window.location.search).get("view") === "list"
      ? "list"
      : readPref(VIEW_KEY, ["grid", "list"] as const, "grid"),
  );
  const searchRef = useRef<HTMLInputElement>(null);

  const setSort = (s: Sort) => { setSortState(s); writePref(SORT_KEY, s); };
  const setView = (v: View) => { setViewState(v); writePref(VIEW_KEY, v); };

  // "/" or Ctrl+F jumps to search, like most launchers.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if ((e.key === "/" && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f")) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const statusOf = (g: GameDefinition): Exclude<Status, "all"> =>
    myLibrary.includes(g.id) ? "library" : installedGames.includes(g.id) ? "detected" : "available";

  // Search applies first; status and genre counts are computed on top of it so
  // every chip shows how many results it would give.
  const searched = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return gameRegistry;
    return gameRegistry.filter(
      (g) =>
        g.label.toLowerCase().includes(q) ||
        g.family.toLowerCase().includes(q) ||
        g.id.toLowerCase().includes(q) ||
        (g.genres ?? []).some((x) => genreLabel(x).toLowerCase().includes(q)),
    );
  }, [gameRegistry, search]);

  const matchesGenres = (g: GameDefinition) => genres.size === 0 || (g.genres ?? []).some((x) => genres.has(x));

  const statusCounts = useMemo(() => {
    const c = { all: 0, library: 0, detected: 0, available: 0 };
    for (const g of searched) {
      if (!matchesGenres(g)) continue;
      c.all++;
      c[statusOf(g)]++;
    }
    return c;
  }, [searched, genres, myLibrary, installedGames]);

  const genreCounts = useMemo(() => {
    const c = new Map<string, number>();
    for (const g of gameRegistry) for (const x of g.genres ?? []) c.set(x, 0);
    for (const g of searched) {
      if (status !== "all" && statusOf(g) !== status) continue;
      for (const x of g.genres ?? []) c.set(x, (c.get(x) ?? 0) + 1);
    }
    return [...c.entries()].sort((a, b) => genreLabel(a[0]).localeCompare(genreLabel(b[0])));
  }, [gameRegistry, searched, status, myLibrary, installedGames]);

  const visible = useMemo(() => {
    const list = searched.filter((g) => matchesGenres(g) && (status === "all" || statusOf(g) === status));
    const byName = (a: GameDefinition, b: GameDefinition) => a.label.localeCompare(b.label);
    return list.sort(sort === "series" ? (a, b) => a.family.localeCompare(b.family) || byName(a, b) : byName);
  }, [searched, genres, status, sort, myLibrary, installedGames]);

  // Launcher shelves ("what's mine / on this PC / everything else") when not
  // filtering by status; a status filter shows one flat list.
  const shelves = useMemo(() => {
    if (status !== "all") return [{ key: status, label: STATUS_LABELS[status], games: visible }];
    const groups: Record<Exclude<Status, "all">, GameDefinition[]> = { library: [], detected: [], available: [] };
    for (const g of visible) groups[statusOf(g)].push(g);
    return (["library", "detected", "available"] as const)
      .map((k) => ({ key: k, label: STATUS_LABELS[k], games: groups[k] }))
      .filter((s) => s.games.length > 0);
  }, [visible, status, myLibrary, installedGames]);

  const filtersActive = search.trim() !== "" || status !== "all" || genres.size > 0;
  const clearFilters = () => {
    setSearch("");
    setStatus("all");
    setGenres(new Set());
  };

  const toggleGenre = (g: string) =>
    setGenres((prev) => {
      const next = new Set(prev);
      if (next.has(g)) next.delete(g);
      else next.add(g);
      return next;
    });

  const handleAdd = async (gameId: string) => {
    try {
      await cmd.addToLibrary(gameId);
      setMyLibrary([...myLibrary, gameId]);
      addLog(`Added ${gameRegistry.find((g) => g.id === gameId)?.label} to library`, "success");
      toastSuccess("Game added to library");
    } catch (e) {
      addLog(`Failed to add game: ${e}`, "error");
      toastError(`Couldn't add the game: ${e}`);
    }
  };

  const handleRemove = async (gameId: string) => {
    try {
      await cmd.removeFromLibrary(gameId);
      setMyLibrary(myLibrary.filter((id) => id !== gameId));
      addLog(`Removed ${gameRegistry.find((g) => g.id === gameId)?.label} from library`, "info");
    } catch (e) {
      addLog(`Failed to remove game: ${e}`, "error");
      toastError(`Couldn't remove the game: ${e}`);
    }
  };

  const itemProps = (game: GameDefinition) => ({
    game,
    inLibrary: myLibrary.includes(game.id),
    detected: installedGames.includes(game.id),
    // A mods/saves folder without install evidence: often left behind by an
    // uninstall, so it's flagged but kept out of the Detected shelf.
    folderFound: !installedGames.includes(game.id) && !!gamePaths[game.id],
    onOpen: () => navigateToGame(game.id),
    onAdd: () => handleAdd(game.id),
    onRemove: () => handleRemove(game.id),
  });

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <SectionHeader
        label={<><b>//</b> {gameRegistry.length} supported games</>}
        title={<>Game <span className="text-neon">browser</span></>}
        description="Find your games and add them to your library. Detected games are already installed on this PC."
        actions={
          <Input
            ref={searchRef}
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setSearch("")}
            placeholder="Search games or genres..."
            aria-label="Search games"
            icon={<Search size={15} />}
            wrapperClassName="w-72"
          />
        }
      />

      {/* Filter console: status, sort and view on one line, genres below */}
      <div className="panel panel-sunken px-4 py-3 space-y-3">
        <div className="flex items-center gap-3 flex-wrap">
          <div className="flex border border-border" role="group" aria-label="Filter by status">
            {(["all", "library", "detected", "available"] as const).map((s) => (
              <Segment key={s} active={status === s} onClick={() => setStatus(s)}>
                {s === "all" ? "All" : s === "library" ? "In library" : s === "detected" ? "Detected" : "Not added"}
                <span className={cx("tabular", status === s ? "text-neon-ink/70" : "text-txt-muted")}>{statusCounts[s]}</span>
              </Segment>
            ))}
          </div>

          <div className="ml-auto flex items-center gap-2">
            <label className="flex items-center gap-2 font-mono text-[10.5px] uppercase tracking-[0.1em] text-txt-muted">
              <ArrowUpDown size={12} />
              <span className="sr-only sm:not-sr-only">Sort</span>
              <select
                value={sort}
                onChange={(e) => setSort(e.target.value as Sort)}
                className="input input-sm !w-auto !py-1 font-mono text-[11px] uppercase tracking-[0.08em]"
                aria-label="Sort games"
              >
                <option value="name">Name A–Z</option>
                <option value="series">Series</option>
              </select>
            </label>
            <div className="flex border border-border" role="group" aria-label="View">
              <Segment active={view === "grid"} onClick={() => setView("grid")} title="Grid view" square>
                <LayoutGrid size={14} />
              </Segment>
              <Segment active={view === "list"} onClick={() => setView("list")} title="List view" square>
                <List size={14} />
              </Segment>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="hud-label mr-1.5"><b>//</b> Genre</span>
          {genreCounts.map(([g, n]) => {
            const on = genres.has(g);
            return (
              <button
                key={g}
                onClick={() => toggleGenre(g)}
                aria-pressed={on}
                disabled={n === 0 && !on}
                className={cx(
                  "tag !py-1 !px-2 transition-colors disabled:opacity-35 disabled:cursor-default",
                  on ? "!text-neon-ink bg-neon !border-neon" : "text-txt-dim hover:text-txt hover:!border-line-hi",
                )}
              >
                {genreLabel(g)}
                <span className={cx("tabular", on ? "opacity-70" : "text-txt-muted")}>{n}</span>
              </button>
            );
          })}
          {filtersActive && (
            <button onClick={clearFilters} className="ml-auto btn btn-ghost btn-sm !text-txt-dim hover:!text-txt">
              <X size={12} />
              Clear filters
            </button>
          )}
        </div>
      </div>

      {shelves.map((shelf) => (
        <section key={shelf.key}>
          <div className="flex items-center gap-3 mb-3">
            <p className="hud-label">
              <b>//</b> {shelf.label}
            </p>
            <span className="font-mono text-[10.5px] text-txt-muted tabular">{String(shelf.games.length).padStart(2, "0")}</span>
            <span className="flex-1 h-px bg-border" />
          </div>
          {view === "grid" ? (
            <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
              {shelf.games.map((game) => <GameTile key={game.id} {...itemProps(game)} />)}
            </div>
          ) : (
            <div className="border border-border divide-y divide-border bg-bg-card">
              {shelf.games.map((game) => <GameRow key={game.id} {...itemProps(game)} />)}
            </div>
          )}
        </section>
      ))}

      {visible.length === 0 && (
        <EmptyState
          icon={<Search size={18} />}
          label="// No match"
          title="No games match these filters"
          description={
            search.trim()
              ? <>Nothing called "{search}". Try the series name, e.g. "sims" or "minecraft".</>
              : "Try another genre or status."
          }
          action={
            <div className="flex gap-2">
              <Button size="sm" onClick={clearFilters}>Clear filters</Button>
              <Button size="sm" variant="ghost" onClick={() => open(REQUEST_GAME_URL).catch(() => {})}>
                Request this game
              </Button>
            </div>
          }
        />
      )}
    </div>
  );
}

const STATUS_LABELS: Record<Status, string> = {
  all: "All games",
  library: "Your library",
  detected: "Detected on this PC",
  available: "More supported games",
};

function Segment({
  active,
  onClick,
  children,
  title,
  square,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
  title?: string;
  square?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      title={title}
      aria-label={title}
      className={cx(
        "flex items-center gap-2 h-8 font-display font-semibold uppercase tracking-[0.06em] text-[11.5px] transition-colors border-r border-border last:border-r-0",
        square ? "w-8 justify-center" : "px-3",
        active ? "bg-neon text-neon-ink" : "text-txt-dim hover:text-txt hover:bg-bg-card-hover",
      )}
    >
      {children}
    </button>
  );
}

interface ItemProps {
  game: GameDefinition;
  inLibrary: boolean;
  detected: boolean;
  folderFound: boolean;
  onOpen: () => void;
  onAdd: () => void;
  onRemove: () => void;
}

function genreLine(game: GameDefinition): string {
  const g = game.genres ?? [];
  return g.length ? g.map(genreLabel).join(" · ") : game.family;
}

function StatusTags({ inLibrary, detected, folderFound }: { inLibrary: boolean; detected: boolean; folderFound: boolean }) {
  return (
    <>
      {inLibrary && <span className="tag text-neon bg-bg/85 backdrop-blur-sm"><Check size={9} />Library</span>}
      {detected && <span className="tag text-status-green bg-bg/85 backdrop-blur-sm"><Radar size={9} />Detected</span>}
      {folderFound && !inLibrary && (
        <span
          className="tag text-txt-muted bg-bg/85 backdrop-blur-sm"
          title="Its mods/saves folder exists, but the game itself doesn't look installed (it may be left over from an uninstall)."
        >
          <Folder size={9} />Folder found
        </span>
      )}
    </>
  );
}

function ItemActions({ inLibrary, onOpen, onAdd, onRemove, compact }: Omit<ItemProps, "game" | "detected" | "folderFound"> & { compact?: boolean }) {
  if (inLibrary) {
    return (
      <div className="flex gap-2">
        <Button size="sm" className={compact ? undefined : "flex-1"} onClick={onOpen} icon={<ArrowRight size={12} />}>
          Open
        </Button>
        <Button variant="ghost" size="sm" className="hover:!text-status-red" onClick={onRemove}>
          Remove
        </Button>
      </div>
    );
  }
  return (
    <Button size="sm" block={!compact} onClick={onAdd} icon={<Plus size={12} />}>
      {compact ? "Add" : "Add to Library"}
    </Button>
  );
}

function GameTile({ game, inLibrary, detected, folderFound, ...actions }: ItemProps) {
  // The registry's hex color tints the tile edge (and the generated cover) so
  // every game reads as its own box, while neon stays reserved for "yours".
  const style = { "--tile": game.primary_color || "rgb(var(--color-accent))" } as CSSProperties;

  return (
    <div
      style={style}
      className={cx(
        "group panel flex flex-col transition-[filter]",
        inLibrary ? "panel-accent" : "hover:[--panel-line:rgb(var(--color-line-hi))]",
      )}
    >
      {/* Art strip: the game's banner (Steam or publisher art, or a custom cover);
          games without art get the generated poster. */}
      <div className="relative aspect-[460/215] overflow-hidden mx-px mt-px border-b border-border bg-bg [clip-path:polygon(calc(var(--cut)-1px)_0,100%_0,100%_100%,0_100%,0_calc(var(--cut)-1px))]">
        <GameArt
          gameId={game.id}
          kind="header"
          className="absolute inset-0"
          imgClassName="saturate-[0.85] group-hover:saturate-100 group-hover:scale-[1.04] !transition-[opacity,transform,filter] !duration-500"
          fallback={<GeneratedCover game={game} />}
        >
          {/* Fade into the card and add faint scanlines so photos sit inside the HUD */}
          <div className="absolute inset-0 bg-gradient-to-t from-bg-card via-bg-card/10 to-transparent" />
          <div className="absolute inset-0 opacity-30 bg-[repeating-linear-gradient(0deg,transparent_0_2px,rgb(0_0_0/0.35)_2px_3px)]" />
        </GameArt>
        <div className="absolute left-0 top-0 bottom-0 w-[3px] bg-[var(--tile)]" />
        <div className="absolute right-3 top-3 flex flex-col items-end gap-1">
          <StatusTags inLibrary={inLibrary} detected={detected} folderFound={folderFound} />
        </div>
      </div>

      <div className="px-4 pt-3 pb-4 flex-1 flex flex-col">
        <p className="font-display font-bold uppercase tracking-[0.03em] text-[15px] leading-tight truncate" title={game.label}>
          {game.label}
        </p>
        <p className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-txt-muted mt-1 truncate" title={genreLine(game)}>
          {genreLine(game)}
        </p>
        <div className="mt-auto pt-3.5">
          <ItemActions inLibrary={inLibrary} {...actions} />
        </div>
      </div>
    </div>
  );
}

/** Dense launcher row for the list view: banner thumb, name, genres, status, actions. */
function GameRow({ game, inLibrary, detected, folderFound, ...actions }: ItemProps) {
  const style = { "--tile": game.primary_color || "rgb(var(--color-accent))" } as CSSProperties;
  return (
    <div style={style} className="group relative flex items-center gap-4 pl-4 pr-3 py-2 hover:bg-bg-card-hover transition-colors">
      <span className={cx("absolute left-0 top-0 bottom-0 w-[2px]", inLibrary ? "bg-neon" : "bg-[var(--tile)] opacity-60")} />
      <div className="relative w-[104px] shrink-0 aspect-[460/215] overflow-hidden border border-border bg-bg">
        <GameArt
          gameId={game.id}
          kind="header"
          className="absolute inset-0"
          imgClassName="saturate-[0.85] group-hover:saturate-100"
          fallback={
            <div className="absolute inset-0 grid place-items-center">
              <div className="absolute inset-0 opacity-30 bg-[radial-gradient(120%_120%_at_100%_0%,var(--tile),transparent_70%)]" />
              <GameIcon iconName={game.icon} size={18} className={cx("relative", game.color)} />
            </div>
          }
        />
      </div>
      <div className="min-w-0 flex-1">
        <p className="font-display font-bold uppercase tracking-[0.03em] text-[14px] leading-tight truncate">{game.label}</p>
        <p className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-txt-muted mt-0.5 truncate">{genreLine(game)}</p>
      </div>
      <div className="hidden md:flex items-center gap-1.5">
        <StatusTags inLibrary={inLibrary} detected={detected} folderFound={folderFound} />
      </div>
      <div className="shrink-0">
        <ItemActions inLibrary={inLibrary} compact {...actions} />
      </div>
    </div>
  );
}

/**
 * Poster-style cover for games without art (or while it loads): the title set
 * large in the display face over a tinted HUD grid, so these tiles look
 * designed next to real box art instead of empty.
 */
function GeneratedCover({ game }: { game: GameDefinition }) {
  return (
    <div className="absolute inset-0">
      <div className="absolute inset-0 opacity-[0.35] bg-[radial-gradient(110%_120%_at_100%_0%,var(--tile),transparent_65%)] group-hover:opacity-50 transition-opacity" />
      <div className="absolute inset-0 opacity-60 bg-[linear-gradient(rgb(var(--color-border)/0.55)_1px,transparent_1px),linear-gradient(90deg,rgb(var(--color-border)/0.55)_1px,transparent_1px)] bg-[size:18px_18px]" />
      <GameIcon iconName={game.icon} size={96} className={cx("absolute -right-3 -bottom-5 opacity-20 group-hover:opacity-30 transition-opacity", game.color)} />
      <div className="absolute left-4 right-10 bottom-3">
        <GameIcon iconName={game.icon} size={16} className={cx("mb-1.5", game.color)} />
        <p className="font-display font-bold uppercase leading-[0.95] tracking-[0.02em] text-[22px] text-txt line-clamp-2 [text-shadow:0_0_24px_var(--tile)]">
          {game.label}
        </p>
      </div>
    </div>
  );
}
