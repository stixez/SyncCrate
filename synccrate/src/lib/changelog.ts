import changelog from "../../../CHANGELOG.md?raw";
import { digest, markReleaseNotesSeen, readReleaseNotesSeen, compareVersions, type Release } from "./releaseNotes";

/** `## 0.8.7 — 2026-10-30` (anything after the date is allowed, like the
 *  release workflow allows it) or `## Unreleased`. */
const HEADING = /^## (?:(\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})\b.*|(Unreleased)\s*)$/;

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

export function releaseFor(version: string): Release | undefined {
  return RELEASES.find((r) => r.version === version);
}

/** The release to announce once after an update, or null. A fresh install
 *  (`newInstall`) has nothing new to announce; someone updating from a
 *  version before release notes existed has never seen any. */
export function releaseToAnnounce(current: string, newInstall: boolean): Release | null {
  const seen = readReleaseNotesSeen();
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
