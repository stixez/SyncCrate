# Contributing

Issues and pull requests are welcome.

## Adding a game

Games live in one [JSON file](synccrate/src-tauri/src/game_registry.json): detection paths, content types and file extensions. No Rust or TypeScript changes needed. Detection strategies are tried in order and the first existing path wins:

| Strategy | Example |
|---|---|
| `documents_relative` | `{"type": "documents_relative", "base": "My Games", "folders": ["Fallout4"]}` |
| `absolute_paths` | `{"type": "absolute_paths", "paths": {"windows": ["%APPDATA%\\VintagestoryData"], "linux": ["~/.config/VintagestoryData"]}}` |
| `steam_library` | `{"type": "steam_library", "folders": ["Fallout 4/Data"]}` (checked in every Steam library) |
| `windows_registry` | `{"type": "windows_registry", "keys": ["HKLM\\SOFTWARE\\Maxis\\The Sims 4"], "value": "Install Dir", "subpath": "Game\\Bin"}` |

Add `"require_any": ["SomeMarker.ini"]` next to `strategies` to only match folders containing one of those files or folders (used for ReShade/GShade). Each content type needs a real subfolder, and its `icon` must exist in `ICON_MAP` in `synccrate/src/components/Sidebar.tsx`. Add `"steam_app_id"` (the number in the game's Steam store URL) so the game gets box art. For games not on Steam, add `"art_urls": {"hero": "https://…"}` pointing to official publisher-hosted key art. Tag the game with `"genres"` from the list in `registry.rs` (`GENRES`). "Detected" also needs install evidence: the Steam app id, or a Windows uninstall / Epic / GOG entry matching the game's `label`. If the installed name differs, add `"install_names": ["World of Warcraft"]`. For games found neither way, add `"install_markers"`: a file relative to the game folder (`"Wow.exe"`), an absolute path (`"%ProgramFiles(x86)%/Game/Game.exe"`) or a registry directory (`"HKLM\\SOFTWARE\\Maxis\\The Sims 4::Install Dir"`). Content type folders must not overlap (one nested in another), which a test checks. A content type with `"extensions": []` takes any file; add `"install_extensions": ["zip"]` so dropped files of that kind still install into it. If the game keeps its own files next to the mods (Bethesda's `Data`, KSP's `GameData`), add `"exclude_patterns"` to the content type so they never sync: `*` and `?` wildcards, a file name without `/` (`"cc???sse*"`) or a path inside the folder with one (`"SquadExpansion/*"`). Set `"disable_method": "rename"` if the game loads mods from subfolders (disabling then renames to `.disabled`), `"none"` if mods are whole folders that can't be toggled per file, and `"duplicate_finder": true` only if identical files in different mod folders are genuinely a problem for the game.

## Building from source

Requires [Node.js](https://nodejs.org/) 18+, [Rust](https://rustup.rs/) stable and the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
git clone https://github.com/stixez/SyncCrate.git
cd SyncCrate/synccrate
npm install
npm run tauri dev     # dev with hot reload
npm run tauri build   # installer in src-tauri/target/release/bundle/
```

Stack: Tauri v2 · Rust · React 19 + TypeScript + Vite · Tailwind CSS · Zustand · TCP transfer with mDNS + UDP discovery.

## Release notes

[CHANGELOG.md](CHANGELOG.md) is the only place to write them. Add changes under `## Unreleased`; when releasing, rename it to `## X.Y.Z — YYYY-MM-DD` (the version in `package.json` and `tauri.conf.json`) before pushing the tag. Start with a one-paragraph summary, then a `### Highlights` list whose items begin with a **bold headline.**

From that one section:
- the release workflow writes the GitHub release notes and the Discord announcement,
- the app's update prompt lists the highlights' headlines,
- after updating, the app shows the summary and first four highlights once, and every version under **Release notes** (the version number at the bottom of the sidebar).
