import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ArrowLeft, ArrowRight, BookOpen, ChevronRight, Home, LifeBuoy, Lightbulb, Package, RefreshCw, Rocket, Search, ShieldCheck, Users, X,
  type LucideIcon,
} from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { GUIDE, searchGuide, type GuideGroup, type GuideSection } from "../lib/guideContent";
import { getGameDef } from "../lib/games";
import { isDemoMode } from "../lib/demoData";
import { toastInfo } from "../lib/toast";
import { openDiscord } from "../lib/links";
import { Button, DiscordLogo, EmptyState, Input, Panel, SectionHeader, cx } from "./ui";
import { inline } from "./Markdown";
import type { Page } from "../lib/types";

/** Pages that don't belong to a game. */
const GLOBAL_PAGES: Page[] = ["crews", "activity", "settings", "game-browser"];

const GROUP_ICONS: Record<string, LucideIcon> = {
  start: Rocket,
  sync: RefreshCw,
  safety: ShieldCheck,
  share: Users,
  mods: Package,
  help: LifeBuoy,
};

/** What people look for most, on the Guide's home. */
const POPULAR = ["connect", "undo", "modpacks", "handoff", "workshop", "fifty-fifty"];

const TOPIC_KEY = "synccrate-guide-topic";

const ALL: { section: GuideSection; group: GuideGroup }[] = GUIDE.flatMap((group) => group.sections.map((section) => ({ section, group })));
const byId = (id: string | null) => ALL.find((e) => e.section.id === id);

function readTopic(): string | null {
  try {
    const demo = isDemoMode() ? new URLSearchParams(window.location.search).get("topic") : null;
    const id = demo ?? localStorage.getItem(TOPIC_KEY);
    return byId(id) ? id : null;
  } catch {
    return null;
  }
}

function saveTopic(id: string | null) {
  try {
    if (id) localStorage.setItem(TOPIC_KEY, id);
    else localStorage.removeItem(TOPIC_KEY);
  } catch {
    // Storage unavailable: the Guide opens on its home next time.
  }
}

function go(page: Page) {
  const s = useAppStore.getState();
  if (GLOBAL_PAGES.includes(page)) {
    s.navigateToGlobal(page);
    return;
  }
  const game = s.myLibrary.includes(s.activeGame) ? s.activeGame : s.myLibrary[0];
  if (!game) {
    toastInfo("Add a game first: click Add Game in the sidebar.");
    s.navigateToGlobal("game-browser");
    return;
  }
  s.navigateToGame(game, page);
}

// ---------------------------------------------------------------------------
// Search helpers

const words = (q: string) => q.toLowerCase().split(/\s+/).filter(Boolean);
const plain = (s: string) => s.replace(/\*\*|`/g, "");

/** `text` with every query word marked. */
function Highlight({ text, terms }: { text: string; terms: string[] }) {
  if (terms.length === 0) return <>{text}</>;
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const parts = text.split(new RegExp(`(${escaped.join("|")})`, "gi"));
  return (
    <>
      {parts.map((p, i) =>
        terms.some((t) => p.toLowerCase() === t) ? <mark key={i} className="bg-neon/20 text-txt px-0.5">{p}</mark> : <span key={i}>{p}</span>,
      )}
    </>
  );
}

/** The step or tip where the search matched, cut around the match. */
function snippet(s: GuideSection, terms: string[]): string | null {
  const first = terms[0];
  if (!first) return null;
  const hit = [...s.steps, s.tip ?? ""].map(plain).find((t) => t.toLowerCase().includes(first));
  if (!hit) return null;
  const at = hit.toLowerCase().indexOf(first);
  const start = Math.max(0, at - 60);
  const end = Math.min(hit.length, at + first.length + 90);
  return `${start > 0 ? "…" : ""}${hit.slice(start, end).trim()}${end < hit.length ? "…" : ""}`;
}

// ---------------------------------------------------------------------------
// Pieces

function TopicTile({ section, group, onOpen, terms = [] }: { section: GuideSection; group: GuideGroup; onOpen: () => void; terms?: string[] }) {
  const Icon = GROUP_ICONS[group.id] ?? BookOpen;
  const snip = snippet(section, terms);
  return (
    <button
      onClick={onOpen}
      className="group w-full text-left box bg-bg-card hover:bg-bg-card-hover hover:border-line-hi transition-colors px-4 py-3.5 flex gap-3"
    >
      <Icon size={15} className="text-txt-muted group-hover:text-neon transition-colors shrink-0 mt-0.5" />
      <span className="min-w-0 flex-1">
        <span className="block font-mono text-[10px] uppercase tracking-[0.1em] text-txt-muted">{group.title}</span>
        <span className="block text-[13.5px] text-txt font-medium mt-0.5"><Highlight text={section.title} terms={terms} /></span>
        <span className="block text-xs text-txt-dim mt-1 leading-relaxed"><Highlight text={section.summary} terms={terms} /></span>
        {snip && <span className="block font-mono text-[11px] text-txt-muted mt-1.5 leading-relaxed"><Highlight text={snip} terms={terms} /></span>}
      </span>
      <ChevronRight size={14} className="text-txt-muted group-hover:text-neon transition-colors shrink-0 mt-0.5" />
    </button>
  );
}

function AskDiscord({ compact }: { compact?: boolean }) {
  return (
    <div className={cx("box bg-bg flex flex-wrap items-center gap-3", compact ? "px-4 py-3" : "px-5 py-4")}>
      <LifeBuoy size={16} className="text-txt-muted shrink-0" />
      <p className="text-[12.5px] text-txt-dim flex-1 min-w-[200px]">
        <span className="text-txt font-medium">Still stuck?</span> Ask on the SyncCrate Discord: people there use the same games.
      </p>
      <Button size="sm" variant="discord" onClick={openDiscord} icon={<DiscordLogo size={13} />}>Ask on Discord</Button>
    </div>
  );
}

function Rail({ groups, topic, searching, onHome, onOpen }: {
  groups: GuideGroup[];
  topic: string | null;
  searching: boolean;
  onHome: () => void;
  onOpen: (id: string) => void;
}) {
  const row = (active: boolean) =>
    cx(
      "w-full text-left flex items-center gap-2 pl-3 pr-2 py-1.5 text-[12.5px] border-l-2 transition-colors",
      active ? "border-l-neon text-txt bg-bg-card" : "border-l-transparent text-txt-dim hover:text-txt hover:bg-bg-card/60",
    );
  return (
    <nav aria-label="Guide topics" className="space-y-4">
      {!searching && (
        <button onClick={onHome} className={row(topic === null)} aria-current={topic === null ? "page" : undefined}>
          <Home size={13} className="shrink-0" /> Overview
        </button>
      )}
      {groups.map((g) => {
        const Icon = GROUP_ICONS[g.id] ?? BookOpen;
        return (
          <div key={g.id}>
            <p className="hud-label flex items-center gap-1.5 mb-1.5 pl-3"><Icon size={11} /> {g.title}</p>
            <ul>
              {g.sections.map((s) => (
                <li key={s.id}>
                  <button onClick={() => onOpen(s.id)} className={row(topic === s.id)} aria-current={topic === s.id ? "page" : undefined}>
                    <span className="truncate">{s.title}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

function Stepper({ steps }: { steps: string[] }) {
  return (
    <ol className="relative">
      {steps.map((step, i) => (
        <li key={i} className="relative flex gap-4 pb-4 last:pb-0">
          {i < steps.length - 1 && <span aria-hidden className="absolute left-[13px] top-7 bottom-0 w-px bg-border" />}
          <span className="relative z-10 w-7 h-7 shrink-0 grid place-items-center border border-line-hi bg-bg font-display text-[12px] text-neon tabular">
            {i + 1}
          </span>
          <p className="text-[13.5px] text-txt-dim leading-relaxed pt-[3px]">{inline(step)}</p>
        </li>
      ))}
    </ol>
  );
}

function Topic({ id, onOpen, onHome }: { id: string; onOpen: (id: string) => void; onHome: () => void }) {
  const entry = byId(id)!;
  const { section, group } = entry;
  const index = ALL.indexOf(entry);
  const prev = ALL[index - 1];
  const next = ALL[index + 1];
  const Icon = GROUP_ICONS[group.id] ?? BookOpen;

  return (
    <article className="space-y-5" aria-labelledby="guide-topic-title">
      <div className="flex items-center gap-1.5 font-mono text-[10.5px] uppercase tracking-[0.1em] text-txt-muted">
        <button onClick={onHome} className="uppercase hover:text-neon transition-colors">Guide</button>
        <ChevronRight size={10} />
        <span className="flex items-center gap-1.5"><Icon size={11} /> {group.title}</span>
      </div>

      <header className="space-y-2">
        <h2 id="guide-topic-title" className="display text-[26px] leading-tight text-txt">{section.title}</h2>
        <p className="text-[14px] text-txt-dim">{section.summary}</p>
      </header>

      <Panel padded bodyClassName="space-y-5">
        <Stepper steps={section.steps} />
        {section.tip && (
          <p className="flex gap-2.5 text-[12.5px] text-txt-dim leading-relaxed border-l-2 border-l-amber bg-amber/5 px-3.5 py-2.5">
            <Lightbulb size={14} className="text-amber shrink-0 mt-0.5" />
            <span>{inline(section.tip)}</span>
          </p>
        )}
        {section.go && (
          <Button variant="primary" onClick={() => go(section.go!.page)} icon={<ArrowRight size={14} />}>
            {section.go.label}
          </Button>
        )}
      </Panel>

      <div className="grid gap-3 sm:grid-cols-2">
        {prev ? (
          <button onClick={() => onOpen(prev.section.id)} className="group box bg-bg hover:bg-bg-card px-4 py-3 text-left transition-colors">
            <span className="flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.1em] text-txt-muted"><ArrowLeft size={11} /> Previous</span>
            <span className="block text-[13px] text-txt-dim group-hover:text-txt mt-1 truncate">{prev.section.title}</span>
          </button>
        ) : <span />}
        {next && (
          <button onClick={() => onOpen(next.section.id)} className="group box bg-bg hover:bg-bg-card px-4 py-3 text-right transition-colors">
            <span className="flex items-center justify-end gap-1.5 font-mono text-[10px] uppercase tracking-[0.1em] text-txt-muted">Next <ArrowRight size={11} /></span>
            <span className="block text-[13px] text-txt-dim group-hover:text-txt mt-1 truncate">{next.section.title}</span>
          </button>
        )}
      </div>

      <AskDiscord compact />
    </article>
  );
}

function Overview({ onOpen }: { onOpen: (id: string) => void }) {
  const activeGame = useAppStore((s) => s.activeGame);
  const library = useAppStore((s) => s.myLibrary);
  useAppStore((s) => s.gameRegistry); // re-render once the registry has loaded
  const gameId = library.includes(activeGame) ? activeGame : null;
  const gameDef = gameId ? getGameDef(gameId) : undefined;

  const first = byId("first-sync")!.section;
  const forGame = ALL.filter(({ section }) =>
    !!gameId && (section.games?.includes(gameId) || (section.id === "workshop" && !!gameDef?.steam_workshop_app_id)),
  );
  const popular = POPULAR.map(byId).filter((e): e is NonNullable<typeof e> => !!e && !forGame.includes(e));

  return (
    <div className="space-y-7">
      <Panel tone="accent" brackets label={<b>// Quick start</b>} title={first.title} icon={<Rocket size={15} className="text-neon" />} bodyClassName="space-y-4">
        <p className="text-[13px] text-txt-dim -mt-1">{first.summary}</p>
        <ol className="grid gap-3 sm:grid-cols-2">
          {first.steps.map((step, i) => (
            <li key={i} className="box bg-bg px-4 py-3 flex gap-3">
              <span className="font-display text-[22px] leading-none text-neon tabular">{i + 1}</span>
              <span className="text-[12.5px] text-txt-dim leading-relaxed">{inline(step)}</span>
            </li>
          ))}
        </ol>
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="primary" onClick={() => go("dashboard")} icon={<ArrowRight size={14} />}>Open the Dashboard</Button>
          <Button variant="ghost" onClick={() => onOpen("first-sync")}>Read the details</Button>
        </div>
      </Panel>

      {forGame.length > 0 && (
        <section className="space-y-3">
          <p className="hud-label"><b>//</b> For {gameDef?.label ?? "your game"}</p>
          <div className="grid gap-3 md:grid-cols-2">
            {forGame.map(({ section, group }) => <TopicTile key={section.id} section={section} group={group} onOpen={() => onOpen(section.id)} />)}
          </div>
        </section>
      )}

      <section className="space-y-3">
        <p className="hud-label"><b>//</b> Common questions</p>
        <div className="grid gap-3 md:grid-cols-2">
          {popular.map(({ section, group }) => <TopicTile key={section.id} section={section} group={group} onOpen={() => onOpen(section.id)} />)}
        </div>
      </section>

      <AskDiscord />
    </div>
  );
}

// ---------------------------------------------------------------------------

export default function Guide() {
  const [query, setQuery] = useState(() => (isDemoMode() ? new URLSearchParams(window.location.search).get("q") ?? "" : ""));
  const [topic, setTopicState] = useState<string | null>(readTopic);
  // While searching, the results show until one is opened.
  const [showResults, setShowResults] = useState(() => query.length > 0);
  const searchRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLElement>(null);

  const terms = useMemo(() => words(query), [query]);
  const groups = useMemo(() => searchGuide(query), [query]);
  const results = useMemo(() => groups.flatMap((g) => g.sections.map((section) => ({ section, group: g }))), [groups]);
  const searching = terms.length > 0;

  const open = (id: string | null) => {
    setTopicState(id);
    setShowResults(false);
    saveTopic(id);
  };

  // A new topic starts at its top, not where the last one was scrolled to.
  useEffect(() => {
    const scroller = contentRef.current?.closest("main") ?? document.scrollingElement;
    if (scroller && contentRef.current && contentRef.current.getBoundingClientRect().top < 0) {
      scroller.scrollTo({ top: 0, behavior: "smooth" });
    }
  }, [topic, showResults]);

  // Keep the open topic visible in the list, which scrolls on its own.
  // Only the list: scrollIntoView also moved the page and hid its header.
  useEffect(() => {
    const rail = railRef.current;
    const el = rail?.querySelector<HTMLElement>('[aria-current="page"]');
    if (!rail || !el || rail.scrollHeight <= rail.clientHeight) return;
    const offset = el.getBoundingClientRect().top - rail.getBoundingClientRect().top;
    if (offset < 0 || offset > rail.clientHeight - el.offsetHeight) rail.scrollTop += offset - rail.clientHeight / 3;
  }, [topic]);

  let body: ReactNode;
  if (searching && showResults) {
    body = results.length === 0 ? (
      <EmptyState
        icon={<BookOpen size={18} />}
        label="// No matches"
        title={`Nothing in the guide mentions "${query.trim()}"`}
        description="Try another word, or ask on the Discord."
        action={<div className="flex gap-2"><Button size="sm" onClick={() => setQuery("")}>Clear search</Button><Button size="sm" variant="discord" onClick={openDiscord} icon={<DiscordLogo size={13} />}>Ask on Discord</Button></div>}
      />
    ) : (
      <div className="space-y-3">
        <p className="hud-label"><b>//</b> {results.length} {results.length === 1 ? "topic" : "topics"} for "{query.trim()}"</p>
        {results.map(({ section, group }) => (
          <TopicTile key={section.id} section={section} group={group} terms={terms} onOpen={() => open(section.id)} />
        ))}
      </div>
    );
  } else if (topic) {
    body = <Topic id={topic} onOpen={open} onHome={() => open(null)} />;
  } else {
    body = <Overview onOpen={open} />;
  }

  return (
    <div className="max-w-[1100px] mx-auto space-y-6">
      <SectionHeader
        label={<><b>// Guide</b> &nbsp;How it works</>}
        title={<>How to use <span className="text-neon">SyncCrate</span></>}
        description="Every task, step by step. Pick a topic, or search for what you want to do."
      />

      <Input
        ref={searchRef}
        value={query}
        onChange={(e) => { setQuery(e.target.value); setShowResults(true); }}
        onKeyDown={(e) => {
          if (e.key === "Escape") { setQuery(""); setShowResults(false); }
          if (e.key === "Enter" && results[0]) { open(results[0].section.id); searchRef.current?.blur(); }
        }}
        placeholder="Search the guide: undo, firewall, Workshop, load order..."
        aria-label="Search the guide"
        icon={<Search size={14} />}
      />
      {searching && (
        <div className="flex items-center gap-3 -mt-3 font-mono text-[11px] text-txt-muted">
          <span>{results.length === 0 ? "No topics match" : <>Press <kbd className="text-txt-dim">Enter</kbd> to open the first result</>}</span>
          <button onClick={() => { setQuery(""); setShowResults(false); }} className="flex items-center gap-1 hover:text-neon transition-colors">
            <X size={11} /> Clear
          </button>
          {!showResults && results.length > 0 && (
            <button onClick={() => setShowResults(true)} className="hover:text-neon transition-colors">Back to results</button>
          )}
        </div>
      )}

      <div className="grid gap-8 md:grid-cols-[220px_minmax(0,1fr)] items-start">
        <aside ref={railRef} className="md:sticky md:top-4 md:max-h-[calc(100vh-7rem)] md:overflow-y-auto pr-1">
          <Rail groups={groups} topic={showResults && searching ? null : topic} searching={searching} onHome={() => open(null)} onOpen={open} />
        </aside>
        <div ref={contentRef} className="min-w-0">{body}</div>
      </div>
    </div>
  );
}
