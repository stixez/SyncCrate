# Frontend

## Demo mode (screenshots, GIFs, previews)

- `npm run dev`, then open `http://localhost:1420/?demo`: the UI runs on demo data (`src/lib/demoData.ts`), no backend needed.
- Variants: `&offline`, `&welcome`, `&light`, `&page=<page>`, `&game=<registry id>` (opens that game, with `&page` on that page), `&big`, `&issues`, `&releasenotes`.
- State can be driven from a headless browser with `await import("/src/stores/useAppStore.ts")` (the same module instance the app uses).
- `scripts/make_gif.sh` turns a recording into a README GIF.

## Release notes in the app

- `src/lib/changelog.ts` bundles `CHANGELOG.md` (`?raw`) and is loaded lazily; `src/lib/releaseNotes.ts` parses it. The **Release notes** page (`ReleaseNotes.tsx`, opened from the version number at the bottom of the sidebar) shows every version; `ReleaseNotesModal.tsx` shows the new version's summary and first four highlights once after an update.
- `src/components/Markdown.tsx` is the one inline Markdown renderer; links open through `openCreatorLink` and must be https.

## Links and copy

- Outside links live in `src/lib/links.ts` (`DISCORD_INVITE`, the website). Buttons that open the Discord use `<Button variant="discord">` with `<DiscordLogo />` (Discord's blurple and logo); the main one sits at the bottom of the sidebar. The Rust side has its own `crate::discord::COMMUNITY` for the Discord status button; change both together.
- Donations: Buy Me a Coffee only, one-time; the app's copy says coffees keep it free. Supporting unlocks nothing.
- The in-app Guide is `src/lib/guideContent.ts`.
- A content type's `icon` must exist in `ICON_MAP` in `src/components/Sidebar.tsx`.
