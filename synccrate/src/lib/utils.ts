export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
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
