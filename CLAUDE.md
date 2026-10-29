# SyncCrate

Free, open-source desktop app (Windows, macOS, Linux) that syncs game mods, custom content and saves between friends' PCs, peer to peer, with no cloud and no accounts. Tauri v2: Rust backend, React frontend.

## Layout

| Path | What |
|------|------|
| `synccrate/src-tauri/src/` | Rust: `commands/` (Tauri commands), `network/` (discovery, join codes, transfer), `sync/` (diffing), `registry.rs` + `game_registry.json` (every supported game), `*_e2e_tests.rs` |
| `synccrate/src/` | React 19 + TypeScript + Vite + Tailwind + Zustand: `components/` (one file per page or panel), `lib/` (commands, guide content, links, release notes), `stores/` |
| `CHANGELOG.md` | The only place release notes are written; the app, GitHub release and Discord post all read it |
| `GAMES.md`, `FEATURES.md`, `FAQ.md`, `CONTRIBUTING.md` | User and contributor docs. `GAMES.md` is generated |
| `scripts/` | `readme_games.py` (writes GAMES.md), `make_gif.sh` (README GIFs), `installer_art.py` |
| `.github/workflows/` | `test.yml` (Windows + Linux), `release.yml` (tag `v*`: build, sign, publish, winget, Discord) |

## Commands

From `synccrate/`:

```bash
npm install
npm run tauri dev        # the app with hot reload
npm run build            # frontend; cargo test needs this first (the binary embeds dist/)
npx tsc --noEmit         # typecheck
```

From `synccrate/src-tauri/`: `cargo test --lib` (fast), `cargo test` (everything, what CI runs).

## Always

- Run the tests for what you touched before calling it done: `cargo test --lib <area>` and `npm run build`.
- User-visible changes get a line under `## Unreleased` in `CHANGELOG.md`.
- Don't push, tag or release without the owner's go. Work happens on branches merged into `main` locally.
- Never print, log or commit tokens, signing keys or webhook URLs.
- User-facing text is plain and short: say what happens and what to click, with UI names in **bold** in docs.

## Read before working on

Each file is short. Open the one that matches the task; don't load them all.

| Task | Read |
|------|------|
| Adding or fixing a game, save folders, detection paths | [.steering/game-registry.md](.steering/game-registry.md) |
| Releasing, CHANGELOG, version bumps, update prompt, release workflow | [.steering/releases.md](.steering/releases.md) |
| Frontend: demo mode, screenshots and GIFs, release notes UI, links, in-app Guide | [.steering/frontend.md](.steering/frontend.md) |
| The Discord server, roles, release posts | [.steering/discord.md](.steering/discord.md) |
| Branches, commit messages, pushing | [.steering/git.md](.steering/git.md) |

Notes that only apply to the owner's machine go in `CLAUDE.local.md` (gitignored).
