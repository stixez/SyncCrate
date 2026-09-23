import { useState, useMemo, type CSSProperties } from "react";
import { Search, Plus, Check, ArrowRight, Radar } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { GameIcon } from "./Sidebar";
import * as cmd from "../lib/commands";
import { toastSuccess } from "../lib/toast";
import type { GameDefinition } from "../lib/types";
import { Button, EmptyState, GameArt, Input, SectionHeader, cx } from "./ui";

export default function GameBrowser() {
  const gameRegistry = useAppStore((s) => s.gameRegistry);
  const myLibrary = useAppStore((s) => s.myLibrary);
  const setMyLibrary = useAppStore((s) => s.setMyLibrary);
  const gamePaths = useAppStore((s) => s.gamePaths);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const addLog = useLogStore((s) => s.addLog);
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!search.trim()) return gameRegistry;
    const q = search.toLowerCase();
    return gameRegistry.filter(
      (g) =>
        g.label.toLowerCase().includes(q) ||
        g.family.toLowerCase().includes(q) ||
        g.id.toLowerCase().includes(q),
    );
  }, [gameRegistry, search]);

  // Launcher shelves instead of one header per family: with ~70 families most would
  // hold a single game, so the family moves onto the tile and the shelves answer
  // "what's mine / what's on this PC / what else is there".
  const shelves = useMemo(() => {
    const library: GameDefinition[] = [];
    const detected: GameDefinition[] = [];
    const rest: GameDefinition[] = [];
    for (const g of filtered) {
      if (myLibrary.includes(g.id)) library.push(g);
      else if (gamePaths[g.id]) detected.push(g);
      else rest.push(g);
    }
    const byLabel = (a: GameDefinition, b: GameDefinition) => a.label.localeCompare(b.label);
    return [
      { key: "library", label: "Your library", games: library },
      { key: "detected", label: "Detected on this PC", games: detected.sort(byLabel) },
      { key: "rest", label: library.length || detected.length ? "More supported games" : "Supported games", games: rest.sort(byLabel) },
    ].filter((s) => s.games.length > 0);
  }, [filtered, myLibrary, gamePaths]);

  const handleAdd = async (gameId: string) => {
    try {
      await cmd.addToLibrary(gameId);
      setMyLibrary([...myLibrary, gameId]);
      addLog(`Added ${gameRegistry.find((g) => g.id === gameId)?.label} to library`, "success");
      toastSuccess("Game added to library");
    } catch (e) {
      addLog(`Failed to add game: ${e}`, "error");
    }
  };

  const handleRemove = async (gameId: string) => {
    try {
      await cmd.removeFromLibrary(gameId);
      setMyLibrary(myLibrary.filter((id) => id !== gameId));
      addLog(`Removed ${gameRegistry.find((g) => g.id === gameId)?.label} from library`, "info");
    } catch (e) {
      addLog(`Failed to remove game: ${e}`, "error");
    }
  };

  return (
    <div className="max-w-5xl mx-auto space-y-7">
      <SectionHeader
        label={<><b>//</b> {gameRegistry.length} supported games</>}
        title={<>Game <span className="text-neon">browser</span></>}
        description="Browse supported games and add them to your library."
        actions={
          <Input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search games..."
            aria-label="Search games"
            icon={<Search size={15} />}
            wrapperClassName="w-72"
          />
        }
      />

      {shelves.map((shelf) => (
        <section key={shelf.key}>
          <div className="flex items-center gap-3 mb-3">
            <p className="hud-label">
              <b>//</b> {shelf.label}
            </p>
            <span className="font-mono text-[10.5px] text-txt-muted tabular">{String(shelf.games.length).padStart(2, "0")}</span>
            <span className="flex-1 h-px bg-border" />
          </div>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
            {shelf.games.map((game) => (
              <GameTile
                key={game.id}
                game={game}
                inLibrary={myLibrary.includes(game.id)}
                detected={!!gamePaths[game.id]}
                onOpen={() => navigateToGame(game.id)}
                onAdd={() => handleAdd(game.id)}
                onRemove={() => handleRemove(game.id)}
              />
            ))}
          </div>
        </section>
      ))}

      {filtered.length === 0 && (
        <EmptyState
          icon={<Search size={18} />}
          label="// No match"
          title="No games match your search"
          description={<>Nothing called "{search}". Try the series name, e.g. "sims" or "minecraft".</>}
          action={<Button size="sm" onClick={() => setSearch("")}>Clear search</Button>}
        />
      )}
    </div>
  );
}

function GameTile({
  game,
  inLibrary,
  detected,
  onOpen,
  onAdd,
  onRemove,
}: {
  game: GameDefinition;
  inLibrary: boolean;
  detected: boolean;
  onOpen: () => void;
  onAdd: () => void;
  onRemove: () => void;
}) {
  // The registry's hex color tints the tile art so every game reads as its own box,
  // while neon stays reserved for "this one is yours".
  const style = { "--tile": game.primary_color || "rgb(var(--color-accent))" } as CSSProperties;
  const types = game.content_types.length;

  return (
    <div
      style={style}
      className={cx(
        "group panel flex flex-col transition-[filter]",
        inLibrary ? "panel-accent" : "hover:[--panel-line:rgb(var(--color-line-hi))]",
      )}
    >
      {/* Art strip: the game's Steam banner (or a custom cover); games without art
          keep the generated tile, tinted with the registry color. */}
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
          {inLibrary && <span className="tag text-neon bg-bg/85 backdrop-blur-sm"><Check size={9} />Library</span>}
          {detected && <span className="tag text-status-green bg-bg/85 backdrop-blur-sm"><Radar size={9} />Detected</span>}
        </div>
      </div>

      <div className="px-4 pt-3 pb-4 flex-1 flex flex-col">
        <p className="font-display font-bold uppercase tracking-[0.03em] text-[15px] leading-tight truncate" title={game.label}>
          {game.label}
        </p>
        <p className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-txt-muted mt-1 truncate">
          {game.family} · {types} content type{types !== 1 ? "s" : ""}
        </p>
        <div className="mt-auto pt-3.5">
          {inLibrary ? (
            <div className="flex gap-2">
              <Button size="sm" className="flex-1" onClick={onOpen} icon={<ArrowRight size={12} />}>
                Open
              </Button>
              <Button variant="ghost" size="sm" className="hover:!text-status-red" onClick={onRemove}>
                Remove
              </Button>
            </div>
          ) : (
            <Button size="sm" block onClick={onAdd} icon={<Plus size={12} />}>
              Add to Library
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * Poster-style cover for games without Steam art (WoW, Minecraft, ...): the
 * title set large in the display face over a tinted HUD grid, so these tiles
 * look designed next to real box art instead of empty.
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
