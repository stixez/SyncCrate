<p align="center">
  <img src="docs/icon.svg" width="72" height="72" alt="SyncCrate" />
</p>

<h1 align="center">SyncCrate</h1>

<p align="center">
  <strong>Same mods on every PC, for co-op squads and LAN parties.</strong>
</p>

<p align="center">
  <a href="../../releases/latest"><img src="https://img.shields.io/github/v/release/stixez/SyncCrate?color=1fb87e&label=download&style=flat-square" alt="Latest Release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-1e2d38?style=flat-square" alt="Platforms" />
  <img src="https://img.shields.io/github/license/stixez/SyncCrate?color=1e2d38&style=flat-square" alt="License" />
  <a href="https://buymeacoffee.com/stixe"><img src="https://img.shields.io/badge/buy%20me%20a%20coffee-support-FFDD00?logo=buymeacoffee&logoColor=black&style=flat-square" alt="Buy Me a Coffee" /></a>
</p>

<p align="center">
  <a href="../../releases/latest"><strong>Download</strong></a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="https://stixez.github.io/SyncCrate">Website</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="#get-started">Get started</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="#supported-games">Games</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="#faq">FAQ</a>
</p>

<p align="center">
  <img src="docs/social-preview.png" width="100%" alt="SyncCrate: Same mods. Every PC. No excuses." />
</p>

One mismatched mod and nobody can join. **SyncCrate makes every PC match the host by copying only the files that differ, straight from PC to PC.** No cloud, no accounts, no tracking.

## Why SyncCrate

- **Everyone can join.** Friends pull the host's exact mods, configs and saves, so nobody is stuck on "version mismatch" in the lobby.
- **Built for LAN parties.** One PC feeds the whole room at 100–900 MB/s. Hosts are found automatically, no internet needed.
- **Or from anywhere.** At home, friends paste one join code. Peer-to-peer and encrypted, no port forwarding or VPN.
- **Nothing breaks.** You see the plan before anything changes, conflicts are yours to decide, every file is verified and backups let you roll back.
- **100 games, real box art.** Valheim, Lethal Company, Minecraft, Baldur's Gate 3, The Sims 4 and more, found on every drive.

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/dashboard.png" alt="Hosting a session: join code, PIN and sync plan" /></td>
    <td width="50%"><img src="docs/screenshots/browser.png" alt="Game browser with box art and genre filters" /></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/content.png" alt="Content page: mods by folder, filters and per-mod toggles" /></td>
    <td width="50%"><img src="docs/screenshots/appearance.png" alt="Appearance settings: accent colors, theme and scale" /></td>
  </tr>
</table>

## Get Started

1. **Download** SyncCrate for Windows, macOS or Linux from [Releases](../../releases/latest) and install it on every PC.
2. **Add your game** in the Game Browser. Its folder is detected automatically, or set it manually.
3. **One friend clicks Start Hosting.** Everyone else picks the host under **Scan for Hosts** (same network) or pastes the join code (anywhere).
4. **Click Compare & Sync**, check the plan, then **Sync Now**.

> [!TIP]
> Can't connect? On the **host** PC click **Fix Windows Firewall** (shown while hosting and in *Network Check*). It asks for admin permission once; SyncCrate itself never needs to run as administrator.

> [!NOTE]
> **macOS:** the app isn't notarized yet, so macOS may call it "damaged". Run `xattr -cr /Applications/SyncCrate.app` once, then open it normally.

Downloads: Windows `.exe` / `.msi` · macOS `.dmg` (Apple Silicon and Intel) · Linux `.AppImage` / `.deb` / `.rpm`. The app updates itself.

<details>
<summary><strong>All features</strong></summary>

**Sync**
- **Only what changed.** SHA-256 diffing, zstd compression (50–80% less data), 100–900 MB/s peer-to-peer on LAN.
- **You decide.** Review the plan with size and time estimate, exclude files or glob patterns, and resolve conflicts per file: keep yours, use theirs, keep both, or keep newer.
- **Resumable.** Cancel or lose the connection mid-sync and it picks up where it left off. Desktop notification when done.

**Connect**
- **Join codes that work anywhere.** The host shares one `SC-…` code and friends paste it, on the same Wi-Fi or across the internet. Reconnecting takes one click.
- **Auto-discovery.** Finds hosts via mDNS and UDP broadcast and tries every address a host has.
- **Multi-peer.** One host, many friends, each syncing independently. Hosts choose which folders peers may sync.

**Mods**
- **Mod manager.** Browse by folder, filter by status, type and tag, and enable or disable one mod or a whole creator folder. 12 tags, drag & drop install, pack/DLC detection and a duplicate finder. Handles 20,000+ files smoothly.
- **Game browser.** 100 games with official box art, filtered by genre and status, in a grid or a list. Set your own cover for any game.
- **Profiles.** Snapshot a setup and share it as a `.synccrate-profile` file on Discord.

**Safety**
- **Backups.** Manual or automatic (before every sync or every 1–24 h) with pruning, plus a safety backup before every restore.
- **Script warnings.** Flags risky files (`.dll`, `.ts4script`, `.lua`, `.jar`) before syncing.
- **Private.** Files never leave your PCs. Warns you if the game is still running.

**Yours**
- **Appearance.** Any accent color (or each game's own), dark/light/system theme, UI scale, compact rows, and effects off for a calmer look.

**Shortcuts:** <kbd>Ctrl/Cmd+Shift+S</kbd> Settings · <kbd>/</kbd> or <kbd>Ctrl+F</kbd> search · <kbd>Esc</kbd> close / clear.

</details>

## Supported Games

<!-- GAMES:START -->
**100 games across 67 families**, defined in a [JSON registry](synccrate/src-tauri/src/game_registry.json). Missing one? [Add it](#adding-a-game), no code needed.

<details>
<summary><strong>Life sim</strong> — 5 games</summary>

| Game | What syncs |
|------|------------|
| **The Sims 2** | Mods & CC, Neighborhoods, Tray Items, Screenshots |
| **The Sims 3** | Mods & CC, Save Files, Tray Items, Screenshots |
| **The Sims 4** | Script Mods, Save Files, Tray Items, Screenshots |
| **The Sims 4 (GShade)** | GShade Presets, GShade Shaders |
| **The Sims 4 (ReShade)** | ReShade Presets, ReShade Shaders, ReShade Presets (Bin) |

</details>

<details>
<summary><strong>Survival & co-op</strong> — 11 games</summary>

| Game | What syncs |
|------|------------|
| **7 Days to Die** | Mods, Save Files, Server Configs |
| **ARK: Survival Evolved** | Mods, Save Files |
| **Barotrauma** | Local Mods |
| **Conan Exiles** | Mods |
| **Don't Starve** | Mods |
| **Don't Starve Together** | Mods |
| **Palworld** | Mods |
| **Project Zomboid** | Mods, Save Files |
| **Subnautica** | Mods (QMods), Save Files |
| **V Rising** | Plugins (BepInEx), Mod Configs |
| **Valheim** | Plugins (BepInEx), Mod Configs |

</details>

<details>
<summary><strong>Sandbox</strong> — 6 games</summary>

| Game | What syncs |
|------|------------|
| **Garry's Mod** | Addons, Maps, Saves |
| **Minecraft Java** | Mods, Worlds, Resource Packs, Shader Packs |
| **Space Engineers** | Mods, Worlds, Blueprints |
| **Starbound** | Mods, Characters, Universe |
| **Terraria** | Worlds, Players, tModLoader Mods, Resource Packs |
| **Vintage Story** | Mods, Save Files, Mod Configs |

</details>

<details>
<summary><strong>Automation</strong> — 3 games</summary>

| Game | What syncs |
|------|------------|
| **Dyson Sphere Program** | Plugins (BepInEx), Mod Configs |
| **Factorio** | Mods, Save Files, Scenarios |
| **Satisfactory** | Mods (SMM) |

</details>

<details>
<summary><strong>RPG</strong> — 15 games</summary>

| Game | What syncs |
|------|------------|
| **Baldur's Gate 3** | Mods, Mod Load Order, Save Files |
| **Cyberpunk 2077** | Archive Mods, Redscript Mods, TweakXL Tweaks, Cyber Engine Tweaks Mods, RED4ext Plugins |
| **Darkest Dungeon** | Mods |
| **Dragon Age: Origins** | Override Mods, AddIns (DAZip), Save Files |
| **Fallout 4** | Mods & Plugins |
| **Fallout: New Vegas** | Mods & Plugins |
| **Kenshi** | Mods, Save Files |
| **Kingdom Come: Deliverance II** | Mods |
| **Skyrim (Legendary Edition)** | Mods & Plugins |
| **Skyrim Special Edition** | Mods & Plugins |
| **Starfield** | Mods & Plugins |
| **The Elder Scrolls III: Morrowind** | Mods & Plugins |
| **The Elder Scrolls IV: Oblivion** | Mods & Plugins |
| **The Witcher 3: Wild Hunt** | Mods, Mod Menus |
| **Torchlight II** | Mods |

</details>

<details>
<summary><strong>MMO</strong> — 7 games</summary>

| Game | What syncs |
|------|------------|
| **WoW Classic** | Addons, Addon Settings |
| **WoW Classic Era** | Addons, Addon Settings |
| **WoW Custom Server** | Addons, Addon Settings |
| **WoW Retail** | Addons, Addon Settings |
| **WoW TBC (2.4.3)** | Addons, Addon Settings |
| **WoW Vanilla (1.12)** | Addons, Addon Settings |
| **WoW WoTLK (3.3.5)** | Addons, Addon Settings |

</details>

<details>
<summary><strong>Strategy</strong> — 16 games</summary>

| Game | What syncs |
|------|------------|
| **Crusader Kings III** | Mods, Save Files |
| **Europa Universalis IV** | Mods, Save Files |
| **Hearts of Iron IV** | Mods, Save Files |
| **Mount & Blade II: Bannerlord** | Modules |
| **Mount & Blade: Warband** | Modules |
| **RimWorld** | Mods, Save Files |
| **Sid Meier's Civilization VI** | Mods, Save Files |
| **Stellaris** | Mods, Save Files |
| **Stronghold 2** | Custom Maps |
| **Stronghold Crusader 2** | Custom Maps |
| **Stronghold HD** | Custom Maps |
| **Stronghold: Crusader HD** | Custom Maps |
| **The Riftbreaker** | Mods |
| **Victoria 3** | Mods, Save Files |
| **Warcraft III** | Custom Maps |
| **XCOM 2** | Mods (Base Game), Mods (War of the Chosen) |

</details>

<details>
<summary><strong>City builders</strong> — 3 games</summary>

| Game | What syncs |
|------|------------|
| **Cities: Skylines** | Mods, Assets, Save Files, Maps |
| **Cities: Skylines II** | Local Mods, Save Files, Maps |
| **SimCity 4** | Plugins, Regions |

</details>

<details>
<summary><strong>Simulation</strong> — 7 games</summary>

| Game | What syncs |
|------|------------|
| **American Truck Simulator** | Mods, Profiles |
| **Euro Truck Simulator 2** | Mods, Profiles |
| **Farming Simulator 22** | Mods |
| **Farming Simulator 25** | Mods |
| **Kerbal Space Program** | Mods, Save Files, Ship Designs |
| **Oxygen Not Included** | Local Mods, Save Files |
| **Stardew Valley** | SMAPI Mods |

</details>

<details>
<summary><strong>Shooters</strong> — 7 games</summary>

| Game | What syncs |
|------|------------|
| **Arma 3** | Missions, MP Missions |
| **Call of Duty** | Mods |
| **Call of Duty 2** | Mods |
| **Call of Duty 4: Modern Warfare** | Mods, Custom Maps |
| **Counter-Strike 2** | Maps, Configs |
| **Left 4 Dead 2** | Addons & Maps, Configs |
| **Team Fortress 2** | Custom (HUDs, Skins, Sounds), Maps, Configs |

</details>

<details>
<summary><strong>Action</strong> — 4 games</summary>

| Game | What syncs |
|------|------------|
| **Blade & Sorcery** | Mods |
| **Grand Theft Auto V** | Scripts, OpenIV Mods Folder |
| **Monster Hunter: World** | nativePC Mods |
| **Red Dead Redemption 2** | Lenny's Mod Loader, Scripts |

</details>

<details>
<summary><strong>Horror</strong> — 2 games</summary>

| Game | What syncs |
|------|------------|
| **Lethal Company** | Plugins (BepInEx), Mod Configs |
| **R.E.P.O.** | Plugins (BepInEx), Mod Configs |

</details>

<details>
<summary><strong>Roguelikes</strong> — 4 games</summary>

| Game | What syncs |
|------|------------|
| **Balatro** | Mods (Steamodded/Lovely), Save Profile 1, Save Profile 2, Save Profile 3 |
| **Noita** | Mods |
| **Risk of Rain 2** | Plugins (BepInEx), Mod Configs |
| **Slay the Spire** | Mods, Save Files, Preferences & Profiles |

</details>

<details>
<summary><strong>Racing</strong> — 5 games</summary>

| Game | What syncs |
|------|------------|
| **Assetto Corsa** | Cars, Tracks |
| **Trackmania (2020)** | Maps, Replays, Skins |
| **TrackMania 2: Stadium** | Maps, Replays, Skins |
| **TrackMania Nations Forever** | Tracks, Replays, Skins |
| **TrackMania United Forever** | Tracks, Replays, Skins |

</details>

<details>
<summary><strong>Platformers</strong> — 2 games</summary>

| Game | What syncs |
|------|------------|
| **Celeste** | Mods (Everest), Save Files |
| **Hollow Knight** | Mods |

</details>

<details>
<summary><strong>Party</strong> — 2 games</summary>

| Game | What syncs |
|------|------------|
| **Among Us** | Plugins (BepInEx), Mod Configs |
| **Tabletop Simulator** | Save Files, Workshop Mods |

</details>

<details>
<summary><strong>Rhythm</strong> — 1 game</summary>

| Game | What syncs |
|------|------------|
| **Beat Saber** | Custom Songs, Plugins (BSIPA), Plugin Libraries, Custom Sabers, Mod Configs |

</details>
<!-- GAMES:END -->

## Game-Specific Extras

**Co-op games with BepInEx** (Valheim, Lethal Company, R.E.P.O., Risk of Rain 2, …): `BepInEx/plugins` and `BepInEx/config` sync as separate folders, so the host can share plugins but keep personal configs. Plugin `.dll` files are flagged as scripts before syncing.

**The Sims 4**
- **ReShade & GShade presets.** Add *The Sims 4 (ReShade)* or *(GShade)* to share shaders and presets so everyone's game looks the same. Detected in `Game\Bin` on EA App, Origin and Steam installs. Your own `ReShade.ini` / `GShade.ini` is never synced.
- **Thumbnail cache** is cleared automatically after new mods arrive.
- **Disabling that sticks.** The game loads mods from subfolders, so disabled mods are renamed to `.disabled` instead of moved. Mods left in an old `_Disabled` folder are fixed in one click.
- **Outdated script mods** are flagged after a game patch.

**LAN party tips:** run the host on wired Ethernet, click **Fix Windows Firewall** on the host before people arrive, and let friends sync in waves (everyone shares the host's upload). Friends can also sync at home the night before using the join code.

## FAQ

<details>
<summary><strong>Does it work at a LAN party with no internet?</strong></summary>

Yes. On a local network SyncCrate finds hosts with mDNS and UDP broadcast and syncs PC to PC at 100–900 MB/s. Friends pick the host under **Scan for Hosts** or use Connect by IP.
</details>

<details>
<summary><strong>How many friends can sync from one host?</strong></summary>

There's no fixed limit. Each friend syncs independently, whenever they're ready, so people can join halfway through the evening. The host's upload is the bottleneck: friends syncing at the same time share it.
</details>

<details>
<summary><strong>My friend can't connect. What do I do?</strong></summary>

It's almost always Windows Firewall. On the host PC click **Fix Windows Firewall**, then use **Test** in *Network Check* to see whether the host is reachable. SyncCrate uses TCP port 9847 for sessions and UDP port 47625 for discovery.
</details>

<details>
<summary><strong>Do I need to run it as administrator?</strong></summary>

No. The only exception is a game folder Windows protects, such as ReShade/GShade under `Program Files`. SyncCrate tells you and offers to restart as administrator.
</details>

<details>
<summary><strong>Can I sync with friends who aren't on my network?</strong></summary>

Yes. Paste the host's join code, and nothing else is needed. SyncCrate connects directly between the two PCs when it can (hole punching through home routers, powered by [iroh](https://iroh.computer)). When that isn't possible, it falls back to an encrypted relay server. The relay only forwards data it can't read, and it only helps set up connections; your files are never stored anywhere. On the same network the fast LAN connection is used automatically.

Anyone with your code can join, so turn on **Use PIN** if you post the code publicly. A virtual LAN like Tailscale or ZeroTier also still works with Connect by IP.
</details>

<details>
<summary><strong>Is there a file size limit?</strong></summary>

Up to 2 GB per file, no limit on total size.
</details>

<details>
<summary><strong>Is it safe? Will it break my mods?</strong></summary>

Nothing changes until you approve the sync plan, and existing files are only replaced when you choose to. Every file is hash-verified after transfer, risky script files are flagged, and backups let you roll back. Only sync with people you trust, and make sure everyone runs the same SyncCrate version.
</details>

<details>
<summary><strong>My game isn't detected or supported.</strong></summary>

Set the folder manually in Settings; any install with the expected folder structure works, Steam or not. To add a new game, see [Adding a game](#adding-a-game).
</details>

## Contributing

Issues and pull requests are welcome.

### Adding a Game

Games live in one [JSON file](synccrate/src-tauri/src/game_registry.json): detection paths, content types and file extensions. No Rust or TypeScript changes needed. Detection strategies are tried in order and the first existing path wins:

| Strategy | Example |
|---|---|
| `documents_relative` | `{"type": "documents_relative", "base": "My Games", "folders": ["Fallout4"]}` |
| `absolute_paths` | `{"type": "absolute_paths", "paths": {"windows": ["%APPDATA%\\VintagestoryData"], "linux": ["~/.config/VintagestoryData"]}}` |
| `steam_library` | `{"type": "steam_library", "folders": ["Fallout 4/Data"]}` (checked in every Steam library) |
| `windows_registry` | `{"type": "windows_registry", "keys": ["HKLM\\SOFTWARE\\Maxis\\The Sims 4"], "value": "Install Dir", "subpath": "Game\\Bin"}` |

Add `"require_any": ["SomeMarker.ini"]` next to `strategies` to only match folders containing one of those files or folders (used for ReShade/GShade). Each content type needs a real subfolder, and its `icon` must exist in `ICON_MAP` in `synccrate/src/components/Sidebar.tsx`. Add `"steam_app_id"` (the number in the game's Steam store URL) so the game gets box art. For games not on Steam, add `"art_urls": {"hero": "https://…"}` pointing to official publisher-hosted key art. Tag the game with `"genres"` from the list in `registry.rs` (`GENRES`).

### Building from Source

Requires [Node.js](https://nodejs.org/) 18+, [Rust](https://rustup.rs/) stable and the [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
git clone https://github.com/stixez/SyncCrate.git
cd SyncCrate/synccrate
npm install
npm run tauri dev     # dev with hot reload
npm run tauri build   # installer in src-tauri/target/release/bundle/
```

Stack: Tauri v2 · Rust · React 19 + TypeScript + Vite · Tailwind CSS · Zustand · TCP transfer with mDNS + UDP discovery.

## Support & License

If SyncCrate saves your game night, consider [buying me a coffee](https://www.buymeacoffee.com/stixe). Released under the [MIT License](LICENSE).
