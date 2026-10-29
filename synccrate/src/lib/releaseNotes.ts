/** One version's section of CHANGELOG.md (parsed in ./changelog, which is
 *  loaded only when needed: the changelog grows with every release). */
export interface Release {
  version: string;
  /** YYYY-MM-DD, or null for the Unreleased section. */
  date: string | null;
  /** Markdown under the version's heading. */
  body: string;
}

/** How many highlights the update popup, update prompt and Discord post show. */
export const HIGHLIGHTS_SHOWN = 4;

/** -1, 0 or 1, comparing dotted numbers ("0.10.0" > "0.9.2"). */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

/** A release in a few lines: its opening paragraph and its first highlights. */
export interface Digest {
  summary: string | null;
  /** Top-level bullet text (markdown), from `### Highlights` when there is one. */
  items: string[];
}

/** Not a summary paragraph: a heading, list item, quote or table row. */
const BLOCK_START = /^(#|[-*] |>|\|)/;

export function digest(body: string, max = HIGHLIGHTS_SHOWN): Digest {
  const lines = body.replace(/\r\n/g, "\n").split("\n");
  const firstBlock: string[] = [];
  for (const l of lines) {
    if (!l.trim()) {
      if (firstBlock.length) break;
      continue;
    }
    firstBlock.push(l.trim());
  }
  const summary = firstBlock.length && !BLOCK_START.test(firstBlock[0]) ? firstBlock.join(" ") : null;

  const bulletsAfter = (start: number) => {
    const out: string[] = [];
    for (let i = start; i < lines.length && out.length < max; i++) {
      const l = lines[i];
      if (/^#{1,3} /.test(l) && out.length) break;
      if (/^- /.test(l)) out.push(l.slice(2).trim());
    }
    return out;
  };
  const hl = lines.findIndex((l) => /^###[^\n]*highlights/i.test(l));
  const items = bulletsAfter(hl >= 0 ? hl + 1 : 0);
  return { summary, items };
}

/** Markdown inline syntax stripped: for toasts and native dialogs. */
export function plain(md: string): string {
  return md
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/(^|\W)\*([^*\s][^*]*)\*(?=\W|$)/g, "$1$2")
    .trim();
}

/** A highlight's headline: its bold lead-in ("**Saves for 28 more games.**"),
 *  else its first sentence. (No lookbehind: older macOS WebKit can't parse it.) */
export function headline(item: string): string {
  const bold = /^\*\*(.+?)\*\*/.exec(item);
  const first = bold ? bold[1] : (/^.*?[.!?](?=\s|$)/.exec(item)?.[0] ?? item);
  return plain(first).replace(/[.:]$/, "");
}

/** What an available update brings, as plain lines for the update prompt.
 *  `body` is the notes the updater got (the version's CHANGELOG section).
 *  Null unless there are highlights or a real summary paragraph. */
export function updateSummary(body: string | null | undefined): string | null {
  if (!body) return null;
  const notes = body.split(/\n---\s*\n|^## Downloads|\n## Downloads/m)[0].replace(/^## .*\n/, "");
  const d = digest(notes);
  if (d.items.length) return d.items.map((i) => `• ${headline(i)}`).join("\n");
  return d.summary ? plain(d.summary) : null;
}

const SEEN_KEY = "synccrate-release-notes-seen";

export function readReleaseNotesSeen(): string | null {
  try {
    return localStorage.getItem(SEEN_KEY);
  } catch {
    return null;
  }
}

export function markReleaseNotesSeen(version: string): void {
  try {
    localStorage.setItem(SEEN_KEY, version);
  } catch {
    // Storage unavailable: the notes may show again, which is harmless.
  }
}
