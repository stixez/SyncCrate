import { useEffect, useState } from "react";
import * as cmd from "./commands";
import type { ModMeta } from "./types";

export const MOD_SOURCE_LABELS: Record<string, string> = {
  thunderstore: "Thunderstore",
  smapi: "SMAPI",
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  paradox: "Paradox mod",
  bannerlord: "Bannerlord module",
  sims4: "Sims 4 CC",
  sims3: "Sims 3 CC",
  rimworld: "RimWorld mod",
  zomboid: "Project Zomboid mod",
  factorio: "Factorio mod",
  darkest: "Darkest Dungeon mod",
  xcom2: "XCOM 2 mod",
  wow: "WoW addon",
  curseforge: "CurseForge",
};

/** The mods' own metadata with CurseForge's (from Check for updates) laid
 * over it: CurseForge's entry for a file replaces the file's own one, which
 * the backend already merged into it. */
export function withCurseForge(metas: ModMeta[], extra: ModMeta[] | undefined): ModMeta[] {
  if (!extra?.length) return metas;
  const keys = new Set(extra.map((m) => m.key));
  return [...metas.filter((m) => !keys.has(m.key)), ...extra];
}

/** Path → the mod it belongs to: a single-file mod (jar) by exact path,
 * otherwise the nearest folder that has metadata. */
export function metaLookup(metas: ModMeta[]): (path: string) => ModMeta | undefined {
  const byKey = new Map(metas.map((m) => [m.key, m]));
  if (byKey.size === 0) return () => undefined;
  return (path: string) => {
    let p = path;
    for (;;) {
      const m = byKey.get(p);
      if (m && (m.is_file ? p === path : true)) return m;
      const slash = p.lastIndexOf("/");
      if (slash < 0) return undefined;
      p = p.slice(0, slash);
    }
  };
}

// One request per mod key; cleared whenever metadata is reloaded.
const iconCache = new Map<string, Promise<string | null>>();

export function clearModIconCache() {
  iconCache.clear();
}

/** The mod's icon as a data: URL (null while loading or when there's none). */
export function useModIcon(meta: ModMeta | undefined): string | null {
  const key = meta?.has_icon ? meta.key : null;
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    if (!key) return;
    let alive = true;
    let p = iconCache.get(key);
    if (!p) {
      p = cmd.getModIcon(key).catch(() => null);
      iconCache.set(key, p);
    }
    p.then((u) => alive && setUrl(u));
    return () => {
      alive = false;
    };
  }, [key]);
  return url;
}
