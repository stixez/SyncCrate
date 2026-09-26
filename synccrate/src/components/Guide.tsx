import { Fragment, useMemo, useState, type ReactNode } from "react";
import { ArrowRight, BookOpen, Lightbulb, Search } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { GUIDE, searchGuide, type GuideSection } from "../lib/guideContent";
import { toastInfo } from "../lib/toast";
import { Button, EmptyState, Input, Panel, SectionHeader, cx } from "./ui";
import type { Page } from "../lib/types";

/** Pages that don't belong to a game. */
const GLOBAL_PAGES: Page[] = ["crews", "activity", "settings", "game-browser"];

/** `**bold**` and `code` in guide text. */
function inline(text: string): ReactNode {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`)/).map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**")) return <b key={i} className="font-medium text-txt">{part.slice(2, -2)}</b>;
    if (part.startsWith("`") && part.endsWith("`")) return <code key={i} className="font-mono text-[11px] text-accent-light bg-bg px-1 border border-border">{part.slice(1, -1)}</code>;
    return <Fragment key={i}>{part}</Fragment>;
  });
}

function go(page: Page) {
  const s = useAppStore.getState();
  if (GLOBAL_PAGES.includes(page)) {
    s.navigateToGlobal(page);
    return;
  }
  const game = s.myLibrary.includes(s.activeGame) ? s.activeGame : s.myLibrary[0];
  if (!game) {
    toastInfo("Add a game first: pick one in the Game Browser.");
    s.navigateToGlobal("game-browser");
    return;
  }
  s.navigateToGame(game, page);
}

function SectionCard({ section }: { section: GuideSection }) {
  return (
    <Panel id={`guide-${section.id}`} title={section.title} bodyClassName="space-y-3" className="scroll-mt-4">
      <p className="text-xs text-txt-dim -mt-1">{section.summary}</p>
      <ol className="text-[12.5px] text-txt-dim leading-relaxed space-y-1.5">
        {section.steps.map((step, i) => (
          <li key={i} className="flex gap-2.5">
            <span className="font-mono text-[10.5px] text-txt-muted tabular shrink-0 pt-[3px]">{String(i + 1).padStart(2, "0")}</span>
            <span>{inline(step)}</span>
          </li>
        ))}
      </ol>
      {section.tip && (
        <p className="flex gap-2 text-[11.5px] text-txt-dim leading-relaxed border-l-2 border-l-amber bg-amber/5 px-3 py-2">
          <Lightbulb size={13} className="text-amber shrink-0 mt-0.5" />
          <span>{inline(section.tip)}</span>
        </p>
      )}
      {section.go && (
        <Button size="sm" variant="ghost" onClick={() => go(section.go!.page)} icon={<ArrowRight size={12} />}>
          {section.go.label}
        </Button>
      )}
    </Panel>
  );
}

export default function Guide() {
  const [query, setQuery] = useState("");
  const groups = useMemo(() => searchGuide(query), [query]);

  const jump = (id: string) => document.getElementById(`guide-group-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });

  return (
    <div className="max-w-[1040px] mx-auto space-y-6">
      <SectionHeader
        label={<><b>// Guide</b> &nbsp;How it works</>}
        title={<>How to use <span className="text-neon">SyncCrate</span></>}
        description="Everything SyncCrate can do, one task at a time. Each section ends with a button that takes you to the right page."
      />

      <div className="space-y-3">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setQuery("")}
          placeholder="Search the guide: undo, firewall, crews, Sims 4..."
          aria-label="Search the guide"
          icon={<Search size={14} />}
        />
        {!query && (
          <div className="flex flex-wrap gap-1.5">
            {GUIDE.map((g) => (
              <button
                key={g.id}
                onClick={() => jump(g.id)}
                className="h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border bg-bg border-line-hi text-txt-dim hover:text-txt transition-colors"
              >
                {g.title}
              </button>
            ))}
          </div>
        )}
      </div>

      {groups.length === 0 ? (
        <EmptyState
          icon={<BookOpen size={18} />}
          label="// No matches"
          title={`Nothing in the guide mentions "${query.trim()}"`}
          description="Try another word, or clear the search to see every section."
          action={<Button size="sm" onClick={() => setQuery("")}>Clear search</Button>}
        />
      ) : (
        groups.map((g) => (
          <section key={g.id} id={`guide-group-${g.id}`} className="space-y-3 scroll-mt-4">
            <p className="hud-label"><b>//</b> {g.title}</p>
            <div className={cx("grid gap-4", g.sections.length > 1 && "md:grid-cols-2")}>
              {g.sections.map((s) => (
                <SectionCard key={s.id} section={s} />
              ))}
            </div>
          </section>
        ))
      )}
    </div>
  );
}
