/*
 * SyncCrate UI primitives — HUD / game-launcher look matching docs/index.html.
 *
 * Tokens (tailwind): bg, bg-2, bg-card(-hover/-active), bg-elevated, border / line, line-hi,
 * txt, txt-dim, txt-muted, accent (game color fill), accent-light, neon (bright accent,
 * per-game, use sparingly), neon-ink (text ON neon), amber (warnings), status-green/yellow/red.
 * Fonts: font-display (Chakra Petch, uppercase headings/numbers/buttons), font-sans (Inter),
 * font-mono (JetBrains Mono: codes, paths, sizes, micro-labels).
 * CSS classes (globals.css): .panel/.panel-accent/-warn/-danger/-sunken (clipped, hairline),
 * .box (flat hairline), .cut, .corner-brackets, .hud-label, .display, .btn + .btn-primary/
 * -secondary/-ghost/-danger + .btn-sm/-lg, .input/.input-mono/.input-sm, .check, .tag,
 * .live-dot(-amber/-idle), .progress > i, .callout, .hud-grid, .scanlines, .grain.
 * No rounded corners, no emoji, no pill badges. Neon = active/live/primary only.
 * Never build class names dynamically (`btn-${v}`): Tailwind only keeps classes it finds
 * literally in source, including the component classes in globals.css.
 *
 * <Button variant="primary|secondary|ghost|danger" size="sm|md|lg" icon={<X size={14}/>} block />
 * <Panel title label icon actions tone="default|accent|warn|danger|sunken" brackets cut padded />
 * <SectionHeader title label="// 01 Session" description actions size="md|lg" />
 * <Badge tone="neutral|neon|green|amber|red" icon dot>v1.2</Badge>
 * <StatTile value={42} label="Mods" hint icon highlight compact />
 * <ProgressBar value={0-100} label meta />
 * <Toggle checked onChange={(v)=>…} label description kind="switch|check" />
 * <Input mono size="sm|md" label icon … native input props />
 * <EmptyState icon title description action label />
 * <Banner tone="info|warn|danger|success" icon title actions>body</Banner>
 * <LiveDot tone="neon|amber|idle" />
 * <GameArt gameId kind="cover|header|hero" fallback={<GeneratedTile/>}>overlays</GameArt>
 * cx(...classes) — tiny classnames helper.
 */
export { default as Button } from "./Button";
export { default as Panel } from "./Panel";
export { default as SectionHeader } from "./SectionHeader";
export { default as Badge } from "./Badge";
export { default as StatTile } from "./StatTile";
export { default as ProgressBar } from "./ProgressBar";
export { default as Toggle } from "./Toggle";
export { default as Input } from "./Input";
export { default as EmptyState } from "./EmptyState";
export { default as Banner } from "./Banner";
export { default as LiveDot } from "./LiveDot";
export { default as GameArt } from "./GameArt";
export { cx } from "./cx";
