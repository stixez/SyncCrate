# FAQ

Didn't find your answer? Ask in #help on the [SyncCrate Discord](https://discord.gg/NY2P9u6v7Z) or [open an issue](https://github.com/stixez/SyncCrate/issues/new/choose).

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

Nothing changes until you approve the sync plan, and existing files are only replaced when you choose to. The duplicate finder only looks at the mods folder, and before deleting a duplicate it re-checks that the copy you keep is still there and identical. Restoring a backup skips mods you've disabled or enabled since. Every file is hash-verified after transfer, risky script files are flagged, and backups let you roll back. Only sync with people you trust, and make sure everyone runs the same SyncCrate version.
</details>

<details>
<summary><strong>My game isn't detected or supported.</strong></summary>

Set the folder manually in Settings; any install with the expected folder structure works, Steam or not. A folder you pick yourself counts as installed. SyncCrate refuses drive roots, your user, Documents, Desktop and Downloads folders, and a folder that overlaps another game's folder. If the folder has none of the game's usual subfolders, it asks before using it. To add a new game, see [Adding a game](CONTRIBUTING.md#adding-a-game).

"Detected" means SyncCrate found the game *installed* (a Steam, Epic or GOG install, or an entry in Windows' installed apps). If only its mods/saves folder exists, for example left behind after an uninstall, SyncCrate doesn't use that folder until you set it yourself.
</details>

<details>
<summary><strong>My tModLoader (Terraria) mods don't show up.</strong></summary>

Mods you subscribed to on the Steam Workshop are kept by Steam in its own folder, not in Terraria's, so SyncCrate can't list, sync or back them up; the Content page tells you how many there are. Friends can subscribe to the same Workshop mods. SyncCrate syncs tModLoader's worlds and players, `enabled.json` (which mods are on) and any `.tmod` files in `Documents/My Games/Terraria/tModLoader/Mods`.
</details>

<details>
<summary><strong>How do I hide a game I don't play any more?</strong></summary>

Settings → Games → **In sidebar** toggles it off (or **Hide them from the sidebar** for every game that isn't installed). Its folder and backups stay; turn it back on the same way. The × next to a game in the sidebar removes it from your library instead.
</details>

<details>
<summary><strong>Where does SyncCrate keep its data? Does uninstalling remove it?</strong></summary>

Profiles, backups, file history, crews and settings live in `%APPDATA%\synccrate` on Windows (`~/Library/Application Support/synccrate` on macOS, `~/.config/synccrate` on Linux). The Windows uninstaller removes it when you tick **Delete the application data**; otherwise it stays so a reinstall picks up where you left off. Updates never remove it.
</details>

<details>
<summary><strong>Settings says "Folder not found — drive disconnected?"</strong></summary>

The folder you set for that game doesn't exist right now, for example because it's on an external drive that isn't plugged in. SyncCrate keeps your setting and won't scan, sync or restore that game until the folder is back. Reconnect the drive, or pick the folder again in Settings.
</details>
