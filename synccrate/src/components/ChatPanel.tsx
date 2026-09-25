import { useEffect, useRef, useState } from "react";
import { MessageSquare, Send } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { Button, Input, Panel, cx } from "./ui";
import { loadDisplayName } from "../lib/prefs";
import * as cmd from "../lib/commands";
import { toastError } from "../lib/toast";
import type { ChatMessage } from "../lib/types";

const MAX_CHARS = 500;

function time(at: number) {
  return new Date(at * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** `@name` mentions of the local user, matched case-insensitively. */
function mentionsMe(text: string, me: string) {
  if (!me) return false;
  return text.toLowerCase().includes(`@${me.toLowerCase()}`);
}

/** Session chat on the dashboard. The backend keeps the log (host) or polls
 * it (client); this only renders the store copy and sends lines. */
export default function ChatPanel() {
  const chat = useAppStore((s) => s.chat);
  const setChat = useAppStore((s) => s.setChat);
  const session = useAppStore((s) => s.session);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const me = loadDisplayName().trim();

  // Initial load for this session (later updates arrive via chat-updated).
  useEffect(() => {
    cmd.getChat().then(setChat).catch(() => {});
  }, [session?.session_type, setChat]);

  const count = (chat?.messages.length ?? 0) + (chat?.outbox.length ?? 0);
  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [count]);

  if (!chat) return null;

  const send = async () => {
    const text = draft.trim();
    if (!text || sending) return;
    setSending(true);
    try {
      setChat(await cmd.sendChat(text));
      setDraft("");
    } catch (e) {
      toastError(`${e}`);
    } finally {
      setSending(false);
    }
  };

  return (
    <Panel label={<><b>// Chat</b> &nbsp;this session</>} title="Session chat" icon={<MessageSquare size={16} className="text-neon" />}>
      {!chat.available ? (
        <p className="text-xs text-txt-dim">Chat needs SyncCrate 0.6 or newer on the host.</p>
      ) : (
        <>
          <div ref={listRef} className="max-h-64 min-h-24 overflow-y-auto space-y-1.5 pr-1" aria-live="polite">
            {chat.messages.length === 0 && chat.outbox.length === 0 && (
              <p className="text-xs text-txt-muted">No messages yet. Say hi, or tell everyone which mods you just added.</p>
            )}
            {chat.messages.map((m) => (
              <Line key={m.seq} m={m} highlight={!m.system && mentionsMe(m.text, me)} />
            ))}
            {chat.outbox.map((text, i) => (
              <p key={`out-${i}`} className="text-[13px] text-txt-muted">
                <span className="font-medium">{me || "You"}</span> {text} <span className="font-mono text-[10px]">· sending…</span>
              </p>
            ))}
          </div>
          <div className="flex gap-2 mt-3">
            <Input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && send()}
              maxLength={MAX_CHARS}
              placeholder="Message everyone in the session..."
              aria-label="Chat message"
            />
            <Button variant="primary" onClick={send} disabled={!draft.trim() || sending} icon={<Send size={14} />}>
              Send
            </Button>
          </div>
          {session?.session_type === "Client" && (
            <p className="text-[11px] text-txt-muted mt-2">Messages go through the host. While a sync is running they wait and arrive right after it.</p>
          )}
        </>
      )}
    </Panel>
  );
}

function Line({ m, highlight }: { m: ChatMessage; highlight: boolean }) {
  if (m.system) {
    return (
      <p className="font-mono text-[11px] text-txt-muted">
        <span className="tabular">{time(m.at)}</span> · {m.text}
      </p>
    );
  }
  return (
    <p className={cx("text-[13px] text-txt-dim break-words", highlight && "border-l-2 border-neon pl-2 text-txt")}>
      <span className="font-mono text-[10px] text-txt-muted tabular mr-1.5">{time(m.at)}</span>
      <span className="font-medium text-txt">{m.from}</span> {m.text}
    </p>
  );
}
