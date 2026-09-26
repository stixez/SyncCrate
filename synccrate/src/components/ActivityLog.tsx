import { useRef, useEffect, useState, useMemo, Fragment } from "react";
import { Trash2, History, ArrowUpDown, ArrowDown, ArrowUp, Terminal, Copy, Download, Search, ArrowDownToLine, ChevronDown, ChevronRight } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { toastAction, toastError, toastSuccess } from "../lib/toast";
import { useLogStore } from "../stores/useLogStore";
import { dirOf, formatBytes, formatDuration } from "../lib/utils";
import { gameLabel } from "../lib/games";
import * as cmd from "../lib/commands";
import type { LogEntry, SyncHistoryEntry } from "../lib/types";
import { Badge, Button, EmptyState, Input, LiveDot, SectionHeader, StatTile, cx } from "./ui";

type LevelFilter = "all" | "problems" | "success" | "info";

const LEVEL_COLOR: Record<LogEntry["level"], string> = {
  info: "text-txt-dim",
  success: "text-status-green",
  warning: "text-amber",
  error: "text-status-red",
};
const LEVEL_TAG: Record<LogEntry["level"], string> = { info: "INFO", success: " OK ", warning: "WARN", error: "ERR " };

const matchesLevel = (l: LogEntry["level"], f: LevelFilter) =>
  f === "all" || (f === "problems" ? l === "error" || l === "warning" : f === "success" ? l === "success" : l === "info");

const time = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const shortTime = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

/** "Today", "Yesterday" or the date: the log keeps 500 lines across restarts, so lines from last week read as today's. */
function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

const logLine = (l: LogEntry) => `${new Date(l.timestamp).toLocaleString()} [${LEVEL_TAG[l.level].trim()}] ${l.message}`;

function directionLabel(d: string) {
  return d === "received" ? "Received" : d === "sent" ? "Sent" : "Synced";
}

export default function ActivityLog() {
  const logs = useLogStore((s) => s.logs);
  const clearLogs = useLogStore((s) => s.clearLogs);
  const [tab, setTab] = useState<"log" | "history">("log");

  // --- Log ---------------------------------------------------------------
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState<LevelFilter>("all");
  const [confirmClearLog, setConfirmClearLog] = useState(false);
  const counts = useMemo(() => {
    const c: Record<LevelFilter, number> = { all: logs.length, problems: 0, success: 0, info: 0 };
    for (const l of logs) {
      if (l.level === "error" || l.level === "warning") c.problems++;
      else if (l.level === "success") c.success++;
      else c.info++;
    }
    return c;
  }, [logs]);
  const shownLogs = useMemo(() => {
    const q = query.trim().toLowerCase();
    return logs.filter((l) => matchesLevel(l.level, level) && (!q || l.message.toLowerCase().includes(q)));
  }, [logs, level, query]);

  // Follow new lines only while at the bottom; scrolled up, a button jumps back.
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const [showJump, setShowJump] = useState(false);
  useEffect(() => {
    const el = scrollRef.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [shownLogs, tab]);
  const jumpToLatest = () => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    atBottom.current = true;
    setShowJump(false);
  };

  const copyLog = () =>
    navigator.clipboard.writeText(shownLogs.map(logLine).join("\n")).then(
      () => toastSuccess(`${shownLogs.length} line${shownLogs.length !== 1 ? "s" : ""} copied`),
      () => toastError("Couldn't copy to the clipboard."),
    );
  const saveLog = async () => {
    try {
      const stamp = new Date().toISOString().slice(0, 10);
      const dest = await save({ defaultPath: `SyncCrate log ${stamp}.txt`, filters: [{ name: "Text", extensions: ["txt", "log"] }] });
      if (!dest) return;
      await cmd.saveTextFile(dest, shownLogs.map(logLine).join("\r\n"));
      toastAction(`Log saved as ${dest.split(/[/\\]/).pop()}`, "Show in folder", () => cmd.openFolder(dirOf(dest)).catch(() => {}));
    } catch (e) {
      toastError(`Couldn't save the log: ${e}`);
    }
  };

  // --- History -------------------------------------------------------------
  const [history, setHistory] = useState<SyncHistoryEntry[]>([]);
  const [historyGame, setHistoryGame] = useState("all");
  const [problemsOnly, setProblemsOnly] = useState(false);
  const [openEntry, setOpenEntry] = useState<string | null>(null);
  const [confirmClearHistory, setConfirmClearHistory] = useState(false);
  useEffect(() => {
    if (tab === "history") cmd.getSyncHistory().then(setHistory).catch(() => {});
  }, [tab]);
  const historyGames = useMemo(() => [...new Set(history.map((h) => h.game))], [history]);
  const shownHistory = useMemo(
    () =>
      history
        .filter((h) => (historyGame === "all" || h.game === historyGame) && (!problemsOnly || h.errors.length > 0 || h.cancelled))
        .slice()
        .reverse(),
    [history, historyGame, problemsOnly],
  );
  const week = useMemo(() => {
    const since = Date.now() / 1000 - 7 * 86400;
    const recent = history.filter((h) => h.timestamp >= since);
    return {
      syncs: recent.length,
      files: recent.reduce((n, h) => n + h.files_synced, 0),
      bytes: recent.reduce((n, h) => n + h.total_bytes, 0),
      problems: recent.filter((h) => h.errors.length > 0).length,
    };
  }, [history]);

  const tabClass = (active: boolean) =>
    cx(
      "flex items-center gap-2 px-3.5 h-9 border-b-2 font-display font-semibold uppercase tracking-[0.06em] text-[12px] transition-colors",
      active ? "border-neon text-txt" : "border-transparent text-txt-muted hover:text-txt hover:border-line-hi",
    );
  const chip = (active: boolean) =>
    cx(
      "h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border transition-colors whitespace-nowrap",
      active ? "bg-neon/10 border-neon text-neon" : "bg-bg border-line-hi text-txt-dim hover:text-txt",
    );

  const confirmRow = (text: string, onYes: () => void, onNo: () => void) => (
    <span className="flex items-center gap-1.5">
      <span className="font-mono text-[11px] uppercase tracking-[0.08em] text-status-red">{text}</span>
      <Button size="sm" variant="danger" onClick={onYes}>Delete</Button>
      <Button size="sm" variant="ghost" onClick={onNo}>Cancel</Button>
    </span>
  );

  return (
    <div className="space-y-4">
      <SectionHeader
        label={<><b>// Activity</b> &nbsp;What SyncCrate did, and every sync</>}
        title="Activity"
        description="The log keeps the last 500 lines, also across restarts. Sync history lists every sync with what came over and what went wrong."
      />

      <div className="flex items-end justify-between gap-4 border-b border-border">
        <div className="flex -mb-px">
          <button onClick={() => setTab("log")} className={tabClass(tab === "log")}>
            <Terminal size={13} />
            Log
            <span className={cx("font-mono font-normal text-[10px] tracking-normal", tab === "log" ? "text-neon" : "text-txt-muted")}>{logs.length}</span>
            {counts.problems > 0 && <span className="font-mono font-normal text-[10px] tracking-normal text-amber">⚠ {counts.problems}</span>}
          </button>
          <button onClick={() => setTab("history")} className={tabClass(tab === "history")}>
            <History size={13} />
            Sync History
          </button>
        </div>
        <div className="pb-1.5">
          {tab === "log" &&
            (confirmClearLog ? (
              confirmRow("Clear the log?", () => { setConfirmClearLog(false); clearLogs(); }, () => setConfirmClearLog(false))
            ) : (
              <span className="flex gap-1.5">
                <Button size="sm" variant="ghost" onClick={copyLog} icon={<Copy size={12} />} disabled={shownLogs.length === 0}>Copy</Button>
                <Button size="sm" variant="ghost" onClick={saveLog} icon={<Download size={12} />} disabled={shownLogs.length === 0}>Save</Button>
                <Button size="sm" variant="ghost" onClick={() => setConfirmClearLog(true)} icon={<Trash2 size={12} />} disabled={logs.length === 0}>Clear</Button>
              </span>
            ))}
          {tab === "history" && history.length > 0 &&
            (confirmClearHistory ? (
              confirmRow("Delete all sync history?", () => {
                setConfirmClearHistory(false);
                cmd.clearSyncHistory().then(() => setHistory([])).catch((e) => toastError(`Couldn't clear it: ${e}`));
              }, () => setConfirmClearHistory(false))
            ) : (
              <Button size="sm" variant="ghost" onClick={() => setConfirmClearHistory(true)} icon={<Trash2 size={12} />}>Clear</Button>
            ))}
        </div>
      </div>

      {tab === "log" && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Input
              size="sm"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setQuery("")}
              placeholder="Search the log..."
              aria-label="Search the log"
              icon={<Search size={12} />}
              wrapperClassName="w-64"
            />
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Show lines">
              {([
                ["all", "All"],
                ["problems", "Problems"],
                ["success", "Done"],
                ["info", "Info"],
              ] as [LevelFilter, string][]).map(([f, label]) => (
                <button key={f} onClick={() => setLevel(f)} aria-pressed={level === f} className={chip(level === f)}>
                  {label} <span className="tabular">{counts[f]}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="panel panel-sunken relative">
            <div className="flex items-center justify-between px-4 h-9 border-b border-border">
              <p className="hud-label flex items-center gap-2">
                <LiveDot tone={logs.length > 0 ? "neon" : "idle"} />
                <span><b>synccrate</b>://session.log</span>
              </p>
              <p className="hud-label tabular">
                {shownLogs.length === logs.length ? `${logs.length} lines` : `${shownLogs.length} of ${logs.length} lines`}
              </p>
            </div>
            <div
              ref={scrollRef}
              onScroll={(e) => {
                const el = e.currentTarget;
                atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
                setShowJump(!atBottom.current);
              }}
              className="max-h-[calc(var(--app-h)-340px)] min-h-40 overflow-y-auto py-2 font-mono text-[12px] leading-[1.6]"
            >
              {logs.length === 0 ? (
                <p className="px-4 py-6 text-txt-muted">
                  <span className="text-neon">&gt;</span> No activity yet<span className="animate-pulse">_</span>
                </p>
              ) : shownLogs.length === 0 ? (
                <p className="px-4 py-6 text-txt-muted">
                  <span className="text-neon">&gt;</span> No line matches.{" "}
                  <button className="text-neon hover:underline" onClick={() => { setQuery(""); setLevel("all"); }}>Show everything</button>
                </p>
              ) : (
                shownLogs.map((log, i) => {
                  const day = dayLabel(log.timestamp);
                  const newDay = i === 0 || dayLabel(shownLogs[i - 1].timestamp) !== day;
                  return (
                    <Fragment key={log.id}>
                      {newDay && (
                        <p className="px-4 pt-2 pb-1 text-[10px] uppercase tracking-[0.12em] text-txt-muted">
                          <span className="text-neon">//</span> {day}
                        </p>
                      )}
                      <div className="group flex items-start gap-3 px-4 py-[3px] hover:bg-bg-card-hover">
                        <span className="text-txt-muted shrink-0 tabular">{time(log.timestamp)}</span>
                        <span className={cx("shrink-0 whitespace-pre", LEVEL_COLOR[log.level])}>[{LEVEL_TAG[log.level]}]</span>
                        <span className={cx("min-w-0 flex-1 break-words select-text", log.level === "info" ? "text-txt" : LEVEL_COLOR[log.level])}>{log.message}</span>
                        <button
                          onClick={() => navigator.clipboard.writeText(logLine(log)).then(() => toastSuccess("Line copied"), () => {})}
                          className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 shrink-0 text-txt-muted hover:text-neon"
                          aria-label="Copy this line"
                          title="Copy this line"
                        >
                          <Copy size={11} />
                        </button>
                      </div>
                    </Fragment>
                  );
                })
              )}
            </div>
            {showJump && (
              <button
                onClick={jumpToLatest}
                className="absolute bottom-3 right-4 flex items-center gap-1.5 h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border border-neon bg-bg text-neon shadow-lg"
              >
                <ArrowDownToLine size={12} /> Latest
              </button>
            )}
          </div>
        </>
      )}

      {tab === "history" && (
        <>
          {history.length > 0 && (
            <div className="grid grid-cols-4 gap-3">
              <StatTile value={week.syncs} label="Syncs this week" highlight />
              <StatTile value={week.files} label="Files" />
              <StatTile value={formatBytes(week.bytes)} label="Data" />
              <StatTile value={week.problems} label="With problems" className={week.problems ? "[&_p]:text-status-red" : undefined} />
            </div>
          )}
          {history.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              {historyGames.length > 1 &&
                ["all", ...historyGames].map((g) => (
                  <button key={g} onClick={() => setHistoryGame(g)} aria-pressed={historyGame === g} className={chip(historyGame === g)}>
                    {g === "all" ? "All games" : gameLabel(g)}
                  </button>
                ))}
              <button onClick={() => setProblemsOnly(!problemsOnly)} aria-pressed={problemsOnly} className={chip(problemsOnly)}>
                Problems only
              </button>
            </div>
          )}
          <div className="box max-h-[calc(var(--app-h)-380px)] min-h-40 overflow-y-auto">
            {history.length === 0 ? (
              <EmptyState
                className="border-0"
                label="// No syncs yet"
                title="Every sync with a friend shows up here"
                description="What was downloaded or sent, how long it took, and anything that went wrong."
              />
            ) : shownHistory.length === 0 ? (
              <p className="px-4 py-6 text-xs text-txt-muted">No sync matches these filters.</p>
            ) : (
              <div>
                {shownHistory.map((entry, i) => {
                  const key = `${entry.timestamp}-${i}`;
                  const day = dayLabel(entry.timestamp * 1000);
                  const newDay = i === 0 || dayLabel(shownHistory[i - 1].timestamp * 1000) !== day;
                  const open = openEntry === key;
                  const speed = entry.duration_ms > 1000 && entry.total_bytes > 0 ? `${formatBytes(entry.total_bytes / (entry.duration_ms / 1000))}/s` : null;
                  return (
                    <Fragment key={key}>
                      {newDay && (
                        <p className="px-4 pt-3 pb-1.5 font-mono text-[10px] uppercase tracking-[0.12em] text-txt-muted border-b border-border bg-bg-2">
                          <span className="text-neon">//</span> {day}
                        </p>
                      )}
                      <div className="border-b border-border last:border-b-0">
                        <button
                          onClick={() => entry.errors.length > 0 && setOpenEntry(open ? null : key)}
                          aria-expanded={entry.errors.length > 0 ? open : undefined}
                          className={cx(
                            "w-full text-left relative flex items-center gap-4 px-4 py-3 hover:bg-bg-card-hover before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px] before:bg-transparent hover:before:bg-neon",
                            entry.errors.length === 0 && "cursor-default",
                          )}
                        >
                          <span className="w-8 h-8 shrink-0 grid place-items-center border border-line-hi text-accent-light">
                            {entry.direction === "received" ? <ArrowDown size={14} /> : entry.direction === "sent" ? <ArrowUp size={14} /> : <ArrowUpDown size={14} />}
                          </span>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="text-[13px] font-medium truncate">
                                {directionLabel(entry.direction)} with <span className="text-txt">{entry.peer_name}</span>
                              </span>
                              {entry.errors.length > 0 && <Badge tone="red">{entry.errors.length} failed</Badge>}
                              {entry.cancelled && <Badge tone="amber">Cancelled</Badge>}
                            </div>
                            <div className="font-mono text-[11px] text-txt-muted mt-0.5">
                              <span className="uppercase tracking-[0.06em]">{gameLabel(entry.game)}</span> &middot;{" "}
                              <span className="text-txt-dim tabular">{entry.files_synced}</span> file{entry.files_synced !== 1 ? "s" : ""} &middot;{" "}
                              <span className="text-txt-dim tabular">{formatBytes(entry.total_bytes)}</span>
                              {!!entry.duration_ms && <> &middot; <span className="text-txt-dim tabular">{formatDuration(entry.duration_ms)}</span></>}
                              {speed && <> &middot; <span className="text-txt-dim tabular">{speed}</span></>}
                            </div>
                          </div>
                          <span className="font-mono text-[11px] text-txt-muted shrink-0">{shortTime(entry.timestamp * 1000)}</span>
                          {entry.errors.length > 0 && (open ? <ChevronDown size={13} className="text-txt-muted shrink-0" /> : <ChevronRight size={13} className="text-txt-muted shrink-0" />)}
                        </button>
                        {open && (
                          <div className="px-4 pb-3 pl-16">
                            <ul className="font-mono text-[11px] text-status-red space-y-0.5 max-h-48 overflow-y-auto select-text">
                              {entry.errors.map((e, j) => <li key={j} className="break-words">{e}</li>)}
                            </ul>
                            <Button
                              size="sm"
                              variant="ghost"
                              className="mt-2"
                              icon={<Copy size={12} />}
                              onClick={() => navigator.clipboard.writeText(entry.errors.join("\n")).then(() => toastSuccess("Errors copied"), () => toastError("Couldn't copy to the clipboard."))}
                            >
                              Copy errors
                            </Button>
                          </div>
                        )}
                      </div>
                    </Fragment>
                  );
                })}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
