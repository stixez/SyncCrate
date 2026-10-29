import { Fragment, type ReactNode } from "react";
import { Lightbulb } from "lucide-react";
import { isHttps, openCreatorLink } from "../lib/links";
import { cx } from "./ui";

/** `**bold**`, `*italic*`, `code` and [links](https://…), for release notes and
 *  the Guide. Links open in the browser (https only). */
export function inline(text: string): ReactNode {
  return text.split(/(\[[^\]]+\]\([^)\s]+\)|\*\*[^*]+\*\*|`[^`]+`|\*[^*\s][^*]*\*)/).map((part, i) => {
    const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(part);
    if (link) {
      const href = link[2];
      if (!isHttps(href)) return <Fragment key={i}>{link[1]}</Fragment>;
      return (
        <a
          key={i}
          href={href}
          onClick={(e) => {
            e.preventDefault();
            openCreatorLink(href);
          }}
          className="text-neon underline underline-offset-2 decoration-neon/40 hover:decoration-neon"
        >
          {link[1]}
        </a>
      );
    }
    if (part.startsWith("**") && part.endsWith("**")) return <b key={i} className="font-medium text-txt">{part.slice(2, -2)}</b>;
    if (part.startsWith("`") && part.endsWith("`")) return <code key={i} className="font-mono text-[11px] text-accent-light bg-bg px-1 border border-border">{part.slice(1, -1)}</code>;
    if (part.length > 2 && part.startsWith("*") && part.endsWith("*")) return <i key={i}>{part.slice(1, -1)}</i>;
    return <Fragment key={i}>{part}</Fragment>;
  });
}

type Block =
  | { kind: "heading"; text: string }
  | { kind: "para"; text: string }
  | { kind: "quote"; text: string }
  | { kind: "list"; items: { text: string; sub: string[]; more: string[] }[] };

/** The small markdown subset release notes use: `###` headings, paragraphs,
 *  `>` quotes and `-` lists (with indented sub-items and paragraphs). */
function parse(md: string): Block[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim() || /^-{3,}\s*$/.test(line)) {
      i++;
    } else if (/^#{1,6}\s/.test(line)) {
      blocks.push({ kind: "heading", text: line.replace(/^#+\s*/, "") });
      i++;
    } else if (line.startsWith(">")) {
      const text: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) text.push(lines[i++].replace(/^>\s?/, ""));
      blocks.push({ kind: "quote", text: text.join(" ") });
    } else if (/^[-*] /.test(line)) {
      const items: { text: string; sub: string[]; more: string[] }[] = [];
      while (i < lines.length) {
        const l = lines[i];
        if (/^[-*] /.test(l)) {
          items.push({ text: l.slice(2).trim(), sub: [], more: [] });
          i++;
        } else if (/^\s{2,}\S/.test(l) && items.length) {
          const t = l.trim();
          const item = items[items.length - 1];
          if (/^[-*] /.test(t)) item.sub.push(t.slice(2));
          else item.more.push(t);
          i++;
        } else if (!l.trim()) {
          // A blank line ends the list unless it continues (indented or another item).
          let j = i + 1;
          while (j < lines.length && !lines[j].trim()) j++;
          if (j < lines.length && (/^\s{2,}\S/.test(lines[j]) || /^[-*] /.test(lines[j]))) i = j;
          else break;
        } else {
          break;
        }
      }
      blocks.push({ kind: "list", items });
    } else {
      const text: string[] = [];
      while (i < lines.length && lines[i].trim() && !/^(#{1,6}\s|>|[-*] )/.test(lines[i])) text.push(lines[i++].trim());
      blocks.push({ kind: "para", text: text.join(" ") });
    }
  }
  return blocks;
}

export default function Markdown({ source, className }: { source: string; className?: string }) {
  return (
    <div className={cx("space-y-3 text-[12.5px] text-txt-dim leading-relaxed", className)}>
      {parse(source).map((b, i) => {
        switch (b.kind) {
          case "heading":
            return (
              <p key={i} className="hud-label pt-2">
                <b>//</b> {b.text}
              </p>
            );
          case "quote":
            return (
              <p key={i} className="flex gap-2 text-[11.5px] border-l-2 border-l-amber bg-amber/5 px-3 py-2">
                <Lightbulb size={13} className="text-amber shrink-0 mt-0.5" />
                <span>{inline(b.text)}</span>
              </p>
            );
          case "list":
            return (
              <ul key={i} className="space-y-1.5">
                {b.items.map((item, k) => (
                  <li key={k} className="flex gap-2.5">
                    <span className="text-neon shrink-0 select-none">▸</span>
                    <div className="min-w-0 space-y-1.5">
                      <p>{inline(item.text)}</p>
                      {item.sub.length > 0 && (
                        <ul className="space-y-1 pl-1">
                          {item.sub.map((s, n) => (
                            <li key={n} className="flex gap-2">
                              <span className="text-txt-muted shrink-0 select-none">–</span>
                              <span>{inline(s)}</span>
                            </li>
                          ))}
                        </ul>
                      )}
                      {item.more.map((m, n) => (
                        <p key={n}>{inline(m)}</p>
                      ))}
                    </div>
                  </li>
                ))}
              </ul>
            );
          default:
            return <p key={i}>{inline(b.text)}</p>;
        }
      })}
    </div>
  );
}
