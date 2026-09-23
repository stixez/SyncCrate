## Adding support for a new game

**Game name:**
**Game family:** <!-- Use an existing family if the game is related (e.g., "wow" for WoW variants), or create a new one -->

### What I did

- [ ] Added a new entry to [`game_registry.json`](../../synccrate/src-tauri/src/game_registry.json)
- [ ] Tested that the game is auto-detected on my machine
- [ ] Verified the content folders exist and files are scanned correctly
- [ ] Added dangerous script extensions if the game uses executable mods (e.g. `dll` for BepInEx)
- [ ] Used `steam_library` detection for mods inside the install folder (forward slashes, e.g. `"Game Name/Mods"`)
- [ ] Added `process_names` (the game's .exe) so SyncCrate can warn when the game is running
- [ ] Added `steam_app_id` (from the Steam store URL) if the game is on Steam, for box art
- [ ] The game `icon` is one of the keys in `ICON_MAP` in `synccrate/src/components/Sidebar.tsx`
- [ ] Every content type has a real subfolder (not `""` / `"."`)

### My registry entry

Paste your JSON entry here. Look at existing games in the registry for reference.

```json

```

### Folder paths I tested

| Platform | Content path |
|----------|-------------|
| Windows  |             |
| macOS    |             |
| Linux    |             |

### How I tested it

Briefly describe what you did to verify it works (e.g., "Added 3 mods, scanned, verified they showed up in Content tab").

### Notes

<!-- Optional: DLC quirks, multiple install locations, special detection logic, etc. -->
