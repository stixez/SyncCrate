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
