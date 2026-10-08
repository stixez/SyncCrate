import type { FileManifest } from "./types";
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

/** Apply toggle results (old path -> new path) to a manifest without a
 * rescan: toggling one mod used to rescan everything and resend the whole
 * file list. The file watcher's rescan catches anything else afterwards. */
export function renameInManifest(m: FileManifest, moves: [string, string][]): FileManifest {
  const files: FileManifest["files"] = { ...m.files };
  for (const [from, to] of moves) {
    const f = files[from];
    if (!f || from === to) continue;
    delete files[from];
    files[to] = { ...f, relative_path: to };
  }
  return { ...m, files };
}

/** A shareable link for a `synccrate://` path (`join/SC-...?game=sims4`).
 * Chat apps only make http(s) links clickable, so this points at the
 * website's open page, which hands the part after `#` to `synccrate://`.
 * Mirrors `open_intent::web_link` in the backend. */
export function webLink(path: string): string {
  return `https://synccrate.app/open/#${path}`;
}

/** The join code (and game, when it names a plausible one) in a pasted invite
 * link: `https://synccrate.app/open/#join/<code>?game=<id>` or
 * `synccrate://join/<code>?game=<id>`. Null for anything else. Same reading
 * as the backend's `open_intent::classify`; the code itself is checked when
 * joining, like a typed one. */
export function parseInviteLink(text: string): { code: string; game: string | null } | null {
  const decode = (s: string) => {
    try {
      return decodeURIComponent(s);
    } catch {
      return s;
    }
  };
  let s = text.trim().replace(/^</, "").replace(/^"+|"+$/g, "");
  if (s.length > 4096) return null;
  // Some chat apps and link shorteners percent-encode the `#`.
  const web = /^https?:\/\/(?:www\.)?synccrate\.app\/open\/?(?:#|%23)(.*)$/i.exec(s);
  if (web) s = `synccrate://${web[1]}`;
  // Chat apps and markdown glue punctuation onto links.
  s = s.replace(/[\s)\]}>.,;:!?"'*|`]+$/, "");
  const m = /^synccrate:\/*join\/+([^?]*)(?:\?(.*))?$/i.exec(s);
  if (!m) return null;
  const code = decode(m[1].replace(/\/+$/, "")).trim().toUpperCase();
  if (!code || code.length > 256) return null;
  const param = (m[2] ?? "")
    .split("&")
    .map((kv) => kv.split("="))
    .find(([k, v]) => v !== undefined && k.toLowerCase() === "game");
  // Browsers sometimes append a `/` to custom-scheme URLs.
  const game = param ? decode(param[1]).trim().replace(/\/+$/, "").toLowerCase() : "";
  return { code, game: /^[a-z0-9_]{1,64}$/.test(game) ? game : null };
}

/** What someone pasted into "join": an invite link (maybe naming the game)
 * or a bare join code. Null when it's neither. */
export function parseJoinInput(text: string): { code: string; game: string | null } | null {
  const link = parseInviteLink(text);
  if (link) return link;
  const t = text.trim().toUpperCase();
  return /^SC[-\s]?[0-9A-Z][0-9A-Z\s-]{8,}$/.test(t) ? { code: t, game: null } : null;
}

/** "1 file" / "3 files" (the "file(s)" style reads like an error message). */
export function plural(n: number, word: string, many = `${word}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? word : many}`;
}

export function formatDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatDateShort(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

/**
 * Whether a mod is disabled: either moved into a `_Disabled/` folder, or
 * renamed with a `.disabled` suffix (The Sims 3/4, which load subfolders).
 */
export function isDisabledPath(relativePath: string): boolean {
  const p = relativePath.replace(/\\/g, "/");
  return p.includes("_Disabled/") || p.toLowerCase().endsWith(".disabled");
}

/** Path as the backend matches it (`diff::match_key`): case-insensitive, with
 * `.disabled`, a legacy `_Disabled/` folder and trailing dots/spaces seen through. */
function linkKey(path: string): string {
  const segs = path.replace(/\\/g, "/").split("/").filter((s) => s && s !== ".").map((s) => s.replace(/[. ]+$/, "").toLowerCase());
  if (segs.length) segs[segs.length - 1] = segs[segs.length - 1].replace(/\.disabled$/, "").replace(/[. ]+$/, "");
  const legacy = segs.slice(0, -1).indexOf("_disabled");
  if (legacy >= 0) segs.splice(legacy, 1);
  return segs.join("/");
}

/** Whether two link prefixes or paths name the same file or folder as the
 * backend matches them. Compare these, not raw paths: a mod under a legacy
 * `_Disabled/` folder has one more segment than its own file link, and was
 * shown as covered by a folder link. */
export function sameLinkTarget(a: string, b: string): boolean {
  return linkKey(a) === linkKey(b);
}

/** The most specific "share as a link" entry covering `path` (a file link beats its folder's). */
export function linkLookup<T extends { prefix: string }>(links: T[]): (path: string) => T | undefined {
  if (links.length === 0) return () => undefined;
  const keyed = links.map((l) => [linkKey(l.prefix), l] as const);
  return (path: string) => {
    const key = linkKey(path);
    let best: readonly [string, T] | undefined;
    for (const k of keyed) {
      if (k[0] && (key === k[0] || key.startsWith(k[0] + "/")) && (!best || k[0].length > best[0].length)) best = k;
    }
    return best?.[1];
  };
}

/** "3d ago" style age for dense tables; pair with formatDate in a title. */
export function formatRelative(ts: number, nowSecs = Date.now() / 1000): string {
  if (!ts) return "—";
  const s = Math.max(0, nowSecs - ts);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d ago`;
  if (s < 86400 * 365) return `${Math.floor(s / (86400 * 30))}mo ago`;
  return `${Math.floor(s / (86400 * 365))}y ago`;
}

export function fileName(relativePath: string): string {
  return relativePath.split(/[/\\]/).pop() || relativePath;
}

/** A manifest path as people read it: `@worlds/Midgard.db` (a folder outside
 * the game folder, see `ContentType::roots`) shows as `Midgard.db`. */
export function displayPath(relativePath: string): string {
  const p = relativePath.replace(/\\/g, "/");
  if (!p.startsWith("@")) return p;
  const i = p.indexOf("/");
  return i < 0 ? "" : p.slice(i + 1);
}

/** Directory part of a relative path with forward slashes ("" for top-level files). */
export function dirOf(relativePath: string): string {
  const p = relativePath.replace(/\\/g, "/");
  const i = p.lastIndexOf("/");
  return i < 0 ? "" : p.slice(0, i);
}

/**
 * Lowercase extension with the dot, ignoring a `.disabled` suffix so a disabled
 * `.package` still counts as a `.package`. "" when the file has none.
 */
export function fileKind(relativePath: string): string {
  let name = fileName(relativePath).toLowerCase();
  if (name.endsWith(".disabled")) name = name.slice(0, -".disabled".length);
  const i = name.lastIndexOf(".");
  return i <= 0 ? "" : name.slice(i);
}

/** "Script mod" only for Sims 4 scripts (.ts4script, or a .zip, which the game
 * loads scripts from); jars, folders and the like are plain mods. */
export function modLabel(relativePath: string, gameId?: string): string {
  const ext = fileKind(relativePath);
  return ext === ".ts4script" || (gameId === "sims4" && ext === ".zip") ? "Script mod" : "Mod";
}

/** "42 s", "3 min 5 s", "1 h 2 min" from milliseconds. */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return s % 60 ? `${m} min ${s % 60} s` : `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}
