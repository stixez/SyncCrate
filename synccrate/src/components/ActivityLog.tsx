import { useRef, useEffect, useState } from "react";
import { Trash2, History, ArrowUpDown, ArrowDown, ArrowUp, Terminal } from "lucide-react";
import { useLogStore } from "../stores/useLogStore";
import { formatBytes } from "../lib/utils";
import { gameLabel } from "../lib/games";
import * as cmd from "../lib/commands";
import type { SyncHistoryEntry } from "../lib/types";
import { Badge, Button, LiveDot, SectionHeader, cx } from "./ui";

export default function ActivityLog() {
  const logs = useLogStore((s) => s.logs);
  const clearLogs = useLogStore((s) => s.clearLogs);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [tab, setTab] = useState<"log" | "history">("log");
  const [history, setHistory] = useState<SyncHistoryEntry[]>([]);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  useEffect(() => {
    if (tab === "history") {
      cmd.getSyncHistory().then(setHistory).catch(() => {});
    }
  }, [tab]);

  const levelColor = {
    info: "text-txt-dim",
    success: "text-status-green",
    warning: "text-amber",
    error: "text-status-red",
  };

  const levelTag = {
    info: "INFO",
    success: " OK ",
    warning: "WARN",
    error: "ERR ",
  };

  const formatTime = (ts: number) => {
    return new Date(ts).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  };

  const formatDate = (ts: number) => {
    return new Date(ts * 1000).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const directionLabel = (d: string) => {
    switch (d) {
      case "received": return "Received";
      case "sent": return "Sent";
      case "bidirectional": return "Synced";
      default: return "Synced";
    }
  };

  const tabClass = (active: boolean) =>
    cx(
      "flex items-center gap-2 px-3.5 h-9 border-b-2 font-display font-semibold uppercase tracking-[0.06em] text-[12px] transition-colors",
      active ? "border-neon text-txt" : "border-transparent text-txt-muted hover:text-txt hover:border-line-hi",
    );

  return (
    <div className="space-y-4">
      <SectionHeader label={<><b>// Activity</b> &nbsp;This session + past syncs</>} title="Activity" />

      <div className="flex items-end justify-between gap-4 border-b border-border">
        <div className="flex -mb-px">
          <button onClick={() => setTab("log")} className={tabClass(tab === "log")}>
            <Terminal size={13} />
            Log
            <span className={cx("font-mono font-normal text-[10px] tracking-normal", tab === "log" ? "text-neon" : "text-txt-muted")}>{logs.length}</span>
          </button>
          <button onClick={() => setTab("history")} className={tabClass(tab === "history")}>
            <History size={13} />
            Sync History
          </button>
        </div>
        <div className="pb-1.5">
          {tab === "log" && (
            <Button size="sm" variant="ghost" onClick={clearLogs} icon={<Trash2 size={12} />}>
              Clear
            </Button>
          )}
          {tab === "history" && history.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                cmd.clearSyncHistory().then(() => setHistory([])).catch(() => {});
              }}
              icon={<Trash2 size={12} />}
            >
              Clear
            </Button>
          )}
        </div>
      </div>

      {tab === "log" && (
        <div className="panel panel-sunken">
          <div className="flex items-center justify-between px-4 h-9 border-b border-border">
            <p className="hud-label flex items-center gap-2">
              <LiveDot tone={logs.length > 0 ? "neon" : "idle"} />
              <span><b>synccrate</b>://session.log</span>
            </p>
            <p className="hud-label tabular">{logs.length} lines</p>
          </div>
          <div ref={scrollRef} className="max-h-[calc(var(--app-h)-290px)] overflow-y-auto py-2 font-mono text-[12px] leading-[1.6]">
            {logs.length === 0 ? (
              <p className="px-4 py-6 text-txt-muted">
                <span className="text-neon">&gt;</span> No activity yet<span className="animate-pulse">_</span>
              </p>
            ) : (
              logs.map((log) => (
                <div key={log.id} className="flex items-start gap-3 px-4 py-[3px] hover:bg-bg-card-hover">
                  <span className="text-txt-muted shrink-0 tabular">{formatTime(log.timestamp)}</span>
                  <span className={cx("shrink-0 whitespace-pre", levelColor[log.level])}>[{levelTag[log.level]}]</span>
                  <span className={cx("min-w-0 break-words", log.level === "info" ? "text-txt" : levelColor[log.level])}>{log.message}</span>
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {tab === "history" && (
        <div className="box max-h-[calc(var(--app-h)-250px)] overflow-y-auto">
          {history.length === 0 ? (
            <div className="px-4 py-6">
              <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-muted">No syncs yet</p>
              <p className="text-xs text-txt-dim mt-1">Every sync with a friend is listed here, with what was downloaded and any problems.</p>
            </div>
          ) : (
            <div className="divide-y divide-border">
              {history.slice().reverse().map((entry, i) => (
                <div
                  key={i}
                  className="relative flex items-center gap-4 px-4 py-3 hover:bg-bg-card-hover before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[2px] before:bg-transparent hover:before:bg-neon"
                >
                  <span className="w-8 h-8 shrink-0 grid place-items-center border border-line-hi text-accent-light">
                    {entry.direction === "received" ? <ArrowDown size={14} /> : entry.direction === "sent" ? <ArrowUp size={14} /> : <ArrowUpDown size={14} />}
                  </span>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium truncate">
                        {directionLabel(entry.direction)} with <span className="text-txt">{entry.peer_name}</span>
                      </span>
                      {entry.errors.length > 0 && <Badge tone="red">{entry.errors.length} error(s)</Badge>}
                    </div>
                    <div className="font-mono text-[11px] text-txt-muted mt-0.5">
                      <span className="uppercase tracking-[0.06em]">{gameLabel(entry.game)}</span> &middot;{" "}
                      <span className="text-txt-dim tabular">{entry.files_synced}</span> file{entry.files_synced !== 1 ? "s" : ""} &middot;{" "}
                      <span className="text-txt-dim tabular">{formatBytes(entry.total_bytes)}</span>
                    </div>
                  </div>
                  <span className="font-mono text-[11px] text-txt-muted shrink-0">{formatDate(entry.timestamp)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
