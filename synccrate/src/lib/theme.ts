type Rgb = [number, number, number];

function hexToRgb(hex: string): Rgb | null {
  const h = hex.replace("#", "");
  if (!/^[0-9a-f]{6}$/i.test(h)) return null;
  return [0, 2, 4].map((i) => parseInt(h.substring(i, i + 2), 16)) as Rgb;
}

function rgbToHsl([r, g, b]: Rgb): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h /= 6;
  return [h, s, l];
}

function hslToRgb(h: number, s: number, l: number): Rgb {
  if (s === 0) { const v = Math.round(l * 255); return [v, v, v]; }
  const hue = (p: number, q: number, t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [hue(p, q, h + 1 / 3), hue(p, q, h), hue(p, q, h - 1 / 3)].map((v) => Math.round(v * 255)) as Rgb;
}

const str = (c: Rgb) => c.join(" ");

/** Relative luminance (WCAG), 0..1. */
function luminance(c: Rgb): number {
  const [r, g, b] = c.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * Derive the theme set from a game's brand color:
 * - accent: the color itself (fills, progress start)
 * - neon: a bright, saturated version that reads on the near-black UI
 * - ink: a dark version for text on the light theme
 * Grey registry colors (e.g. "#9ca3af") stay grey instead of turning neon.
 */
function deriveTheme(rgb: Rgb) {
  const [h, s, l] = rgbToHsl(rgb);
  const chromatic = s > 0.18;
  let neon = hslToRgb(h, chromatic ? Math.max(s, 0.85) : s, Math.max(l, 0.62));
  // Keep neon bright enough for dark text on it and for contrast on #07090b.
  for (let i = 0; i < 6 && luminance(neon) < 0.36; i++) {
    const [nh, ns, nl] = rgbToHsl(neon);
    neon = hslToRgb(nh, ns, Math.min(nl + 0.05, 0.85));
  }
  let ink = hslToRgb(h, chromatic ? Math.max(s, 0.7) : s, Math.min(l, 0.36));
  for (let i = 0; i < 6 && luminance(ink) > 0.16; i++) {
    const [ih, is, il] = rgbToHsl(ink);
    ink = hslToRgb(ih, is, il - 0.04);
  }
  const light = hslToRgb(h, s, Math.min(l + 0.12, 0.7));
  return { accent: rgb, accentLight: light, neon, ink };
}

const VARS = ["--game-accent", "--game-accent-light", "--game-neon", "--game-neon-ink", "--game-accent-ink"];

/** SyncCrate's own green: keep the exact website neon rather than a derived one. */
const BRAND = "#1ea84b";

/** Apply a game's primary color as the global accent. Pass null to reset to the brand. */
export function applyGameTheme(hexColor: string | null): void {
  const root = document.documentElement;
  const rgb = hexColor ? hexToRgb(hexColor) : null;
  if (!rgb || hexColor!.toLowerCase() === BRAND) {
    for (const v of VARS) root.style.removeProperty(v);
    return;
  }
  const t = deriveTheme(rgb);
  root.style.setProperty("--game-accent", str(t.accent));
  root.style.setProperty("--game-accent-light", str(t.accentLight));
  root.style.setProperty("--game-neon", str(t.neon));
  root.style.setProperty("--game-neon-ink", str(t.ink));
  root.style.setProperty("--game-accent-ink", str(t.ink));
}
