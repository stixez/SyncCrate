import { useEffect, useState } from "react";
import { applyGameTheme } from "./theme";
import { loadAppearance, loadThemeMode, type Appearance, type ThemeMode } from "./prefs";

/**
 * Curated accents. Each one is run through theme.ts deriveTheme like any custom
 * color, so they're picked as mid-lightness, saturated tones that produce a clean
 * neon on the near-black UI and a readable ink on the light theme.
 */
export const ACCENT_PRESETS: { name: string; hex: string }[] = [
  { name: "Mint", hex: "#1fb87e" },
  { name: "Cyan", hex: "#12a8c4" },
  { name: "Cobalt", hex: "#3d7bfd" },
  { name: "Violet", hex: "#8b5cf6" },
  { name: "Rose", hex: "#e8457e" },
  { name: "Crimson", hex: "#e5484d" },
  { name: "Amber", hex: "#f0a020" },
  { name: "Lime", hex: "#8fcc1a" },
];

const darkQuery = () => window.matchMedia?.("(prefers-color-scheme: dark)");
const reducedMotionQuery = () => window.matchMedia?.("(prefers-reduced-motion: reduce)");

export function resolveTheme(mode: ThemeMode): "dark" | "light" {
  if (mode !== "system") return mode;
  // No matchMedia (very old webview) → the app's native look.
  const q = darkQuery();
  return q && !q.matches ? "light" : "dark";
}

export function applyThemeClass(mode: ThemeMode) {
  document.documentElement.classList.toggle("light", resolveTheme(mode) === "light");
}

export function effectsEnabled(a: Appearance): boolean {
  return a.effects ?? !reducedMotionQuery()?.matches;
}

/** Root classes/vars for scale, density and effects. Accent is separate (it depends on the selected game). */
export function applyAppearanceRoot(a: Appearance) {
  const root = document.documentElement;
  root.style.setProperty("--ui-scale", String(a.scale));
  root.classList.toggle("density-compact", a.density === "compact");
  root.classList.toggle("fx-off", !effectsEnabled(a));
}

export function applyAccent(hex: string) {
  applyGameTheme(hex);
}

/**
 * Called from main.tsx before React renders so a light theme, custom accent or
 * scale doesn't flash in after the first paint. "Match game color" is resolved
 * later by Layout once the selected game is known.
 */
export function bootAppearance() {
  const a = loadAppearance();
  applyThemeClass(loadThemeMode());
  applyAppearanceRoot(a);
  applyAccent(a.accent);
}

/** Re-render when the OS color scheme / reduced-motion setting changes (for "system" and auto effects). */
export function useMediaPreference(): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const queries = [darkQuery(), reducedMotionQuery()].filter((q): q is MediaQueryList => !!q);
    const onChange = () => setTick((t) => t + 1);
    queries.forEach((q) => q.addEventListener("change", onChange));
    return () => queries.forEach((q) => q.removeEventListener("change", onChange));
  }, []);
  return tick;
}

/** True when dark marks read better than white ones on this color (swatch check icons). */
export function isLightColor(hex: string): boolean {
  const n = parseInt(hex.slice(1), 16);
  if (Number.isNaN(n)) return false;
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  // Perceived brightness (ITU-R BT.601); cheaper than WCAG and plenty for an icon.
  return r * 0.299 + g * 0.587 + b * 0.114 > 150;
}
