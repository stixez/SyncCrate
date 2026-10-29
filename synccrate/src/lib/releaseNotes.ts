import changelog from "../../../CHANGELOG.md?raw";

/** One version's section of CHANGELOG.md (the repo root, bundled at build time). */
export interface Release {
  version: string;
  /** YYYY-MM-DD, or null for the Unreleased section. */
  date: string | null;
  /** Markdown under the version's heading. */
  body: string;
}

const HEADING = /^## (?:(\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})|(Unreleased))\s*$/;

export function parseChangelog(text: string): Release[] {
  const releases: Release[] = [];
  let current: Release | null = null;
  let lines: string[] = [];
  const flush = () => {
    if (current) releases.push({ ...current, body: lines.join("\n").trim() });
  };
  for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
    const m = HEADING.exec(line);
    if (m) {
      flush();
      current = m[3] ? { version: "Unreleased", date: null, body: "" } : { version: m[1], date: m[2], body: "" };
      lines = [];
    } else if (current) {
      lines.push(line);
    }
  }
  flush();
  return releases;
}

/** Released versions, newest first (Unreleased is never shown in the app). */
export const RELEASES: Release[] = parseChangelog(changelog).filter((r) => r.date !== null);

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

export function releaseFor(version: string): Release | undefined {
  return RELEASES.find((r) => r.version === version);
}

/** A release in a few lines: its opening paragraph and its first highlights. */
export interface Digest {
  summary: string | null;
  /** Top-level bullet text (markdown), from `### Highlights` when there is one. */
  items: string[];
}

export function digest(body: string, max = 4): Digest {
  const lines = body.replace(/\r\n/g, "\n").split("\n");
  const firstBlock: string[] = [];
  for (const l of lines) {
    if (!l.trim()) {
      if (firstBlock.length) break;
      continue;
    }
    firstBlock.push(l.trim());
  }
  const summary = firstBlock.length && !/^(#|-|>|\*)/.test(firstBlock[0]) ? firstBlock.join(" ") : null;

  const bulletsAfter = (start: number) => {
    const out: string[] = [];
    for (let i = start; i < lines.length && out.length < max; i++) {
      const l = lines[i];
      if (/^#{1,3} /.test(l) && out.length) break;
      if (/^- /.test(l)) out.push(l.slice(2).trim());
    }
    return out;
  };
  const hl = lines.findIndex((l) => /^###\s+.*highlights/i.test(l));
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
 *  else its first sentence. */
export function headline(item: string): string {
  const bold = /^\*\*(.+?)\*\*/.exec(item);
  const text = plain(bold ? bold[1] : item.split(/(?<=[.!?])\s/)[0]);
  return text.replace(/[.:]$/, "");
}

/** What an available update brings, as plain text for the update prompt.
 *  `body` is the release notes the updater got (the GitHub release). */
export function updateSummary(body: string | null | undefined): string | null {
  if (!body) return null;
  const notes = body.split(/\n---\s*\n|\n## Downloads/)[0].replace(/^## .*\n/, "");
  const d = digest(notes, 4);
  if (d.items.length) return d.items.map((i) => `• ${headline(i)}`).join("\n");
  return d.summary ? plain(d.summary) : null;
}

const SEEN_KEY = "synccrate-release-notes-seen";

function readSeen(): string | null {
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

/** The release to announce once after an update, or null. A fresh install
 *  (`newInstall`) has nothing new to announce; someone updating from a
 *  version before release notes existed has never seen any. */
export function releaseToAnnounce(current: string, newInstall: boolean): Release | null {
  const seen = readSeen();
  if (seen === null && newInstall) {
    markReleaseNotesSeen(current);
    return null;
  }
  if (seen !== null && compareVersions(current, seen) <= 0) return null;
  const release = releaseFor(current);
  const d = release ? digest(release.body) : null;
  if (!release || !d || (!d.summary && d.items.length === 0)) {
    markReleaseNotesSeen(current);
    return null;
  }
  return release;
}
