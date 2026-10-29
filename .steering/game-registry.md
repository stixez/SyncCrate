# Game registry

Every supported game is one entry in `synccrate/src-tauri/src/game_registry.json`: detection, content types (folders) and file types. The format is in CONTRIBUTING.md ("Adding a game"); this file is how to get the facts right.

## Verify facts at the source

- Use the game's Steam config, not search snippets: `curl -s https://api.steamcmd.net/v1/info/<appid>` gives `config.installdir` (the `steam_library` folder), `config.launch.*.executable` (`process_names`), `common.oslist`, and `ufs.savefiles` + `ufs.rootoverrides` (the exact Steam Cloud save folders per OS).
- PCGamingWiki blocks automated fetches; don't try to get around it.
- `steam_config_facts_stay_fixed` (registry tests) pins verified facts. Add new ones there.

## Saves

- A save path containing `{64BitSteamID}` or `{Steam3AccountID}` is per Steam account: don't add it. A friend's game only reads its own account's folder. Those games sync mods, not saves (FAQ and the site say so).
- Saves outside the game folder use `roots`: Windows, then macOS, then Linux candidates; the first existing one wins.
- Write Windows Documents as `%DOCUMENTS%` (resolved like `documents_relative`, so OneDrive and moved Documents work). It's Windows-only, so give macOS and Linux their own entries. Never `%USERPROFILE%/Documents`.
- An external save type with `"extensions": []` must exclude `steam_autocloud.vdf` (Steam's account file). A test enforces it.

## Mods

- Never sync a game's own files. Unreal pak mods go in `Content/Paks/~mods` or `LogicMods`, never `Paks` itself; Bethesda `Data` and KSP `GameData` need `exclude_patterns` for the base game files.

## Editing

- Edit the JSON with targeted text edits. `json.dump` reformats the whole hand-formatted file.
- After a change: `cargo test --lib registry` (in `synccrate/src-tauri`), then `python scripts/readme_games.py` (writes GAMES.md).

## Adding games changes counts in

- `README.md` ("and N more"), `FEATURES.md` (game browser), the minimum in `registry_includes_expanded_game_catalog`, `CHANGELOG.md`.
- Discord: a tag in #share-your-setup (see [discord.md](discord.md)).
