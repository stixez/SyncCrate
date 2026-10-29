# Changelog

Every SyncCrate release, newest first. The app shows a version's notes once after it updates, and the release workflow copies the section into the GitHub release and the Discord announcement.

Releasing: rename `## Unreleased` to `## X.Y.Z — YYYY-MM-DD` before tagging. Start with a one-paragraph summary, then `### Highlights` (the update popup shows its first four items).

## Unreleased

Five new co-op games, saves for 28 more, and SyncCrate now tells you what changed after it updates. It supports 129 games.

### Highlights
- **Five new games.** Schedule I (MelonLoader mods and their libraries), Sons of the Forest (RedLoader), Raft (Raft Mod Loader), Abiotic Factor (pak, Logic and UE4SS mods) and Grounded (pak mods) join the list, so a co-op group can get the host's exact mods in one sync.
- **Saves for 28 more games.** Skyrim, Fallout 3, 4 and New Vegas, Oblivion (and Remastered), Morrowind, The Witcher 3, Cyberpunk 2077, XCOM 2, Bannerlord, Kingdom Come 1 and 2, Hollow Knight, Cult of the Lamb, R.E.P.O., PEAK, Barotrauma and more now sync their saves along with the mods. Valheim characters and Dyson Sphere Program blueprints sync too. The folders come from each game's own Steam Cloud setup.
- **Release notes in the app.** After an update, a short summary of what changed shows once. Every version's notes are a click away: click the version number at the bottom of the sidebar, or open Settings → Application.
- **See what an update brings before installing it.** The update prompt lists the new version's highlights.

### Fixes
- Euro Truck Simulator 2 and American Truck Simulator profiles kept by Steam Cloud (`steam_profiles`) now sync.
- Celeste saves sync on macOS and Linux, 7 Days to Die mods in your user folder sync, Heroes of Might and Magic III's Steam HD Edition is found, and Torchlight II is found on macOS and Linux.
- SyncCrate knows when RimWorld 1.6, PAYDAY 2 and 64-bit Garry's Mod are running.

## 0.8.6 — 2026-10-20

Modded Minecraft gets the whole modpack across, not just the jars, and BepInEx games now bring the patchers some mods need to load.

### Highlights
- **Minecraft modpacks sync completely.** Mod configs (`config`, `defaultconfigs`), global data packs (`datapacks`), KubeJS scripts (`kubejs`) and CraftTweaker scripts (`scripts`) now sync with the mods, so friends get the same recipes and settings as the host. Thanks to @madmaxx11 for the request (#4).
- **Your own Minecraft settings stay yours.** `options.txt`, video mod settings (Sodium, Embeddium), your shader choice and shader options, voice chat devices, minimap settings and saved logins are never copied. The Distant Horizons cache stays local too.
- **Unpacked resource and shader packs.** Packs that are folders instead of `.zip` files sync now. Dropping a shader pack `.zip` onto SyncCrate puts it in `shaderpacks`, not `resourcepacks`.
- **BepInEx patchers.** Valheim, Lethal Company, R.E.P.O., PEAK, Risk of Rain 2, V Rising and the other BepInEx games now sync `BepInEx/patchers`, so mods that need a preloader patcher load on friends' PCs too.
- **More mod settings.** Subnautica now syncs its mod configs, and Valheim's include `.json` and `.yml` files.

Everyone in the group should update: a friend still on 0.8.5 only gets the folders their version knows about.

### Smaller improvements
- KubeJS (`.js`) and CraftTweaker (`.zs`) scripts run code, so they're flagged before they arrive, like `.jar` mods, and wait for a manual sync when Stay in sync is on. `.js` files are still refused everywhere else.
- Files SyncCrate skips no longer make it rescan while you play.
- Using a modpack from the CurseForge app, Prism or the Modrinth app? In Settings, **Browse** to the folder that holds `mods` (for Prism, the `minecraft` folder inside the instance). The in-app Guide explains it.

> **Tip:** some mods rewrite their config files every time the game starts. After playing, those can show up as changes the next time you sync; keeping the host's copy is usually right.

## 0.8.5 — 2026-10-08

Sharing gets easier and safer: invite and pack links you can click in Discord, real mod info and updates from CurseForge, a way to respect creators who don't allow reuploads, and a first run that asks the one question that matters: are you hosting or joining?

### Highlights
- **Clickable invite and pack links.** Copy invite link, modpack links and crew invites are now normal web links (synccrate.app/open/…), so they're clickable in Discord and chat apps. They open SyncCrate, or show what the invite or pack is with a download button for friends who don't have it yet. Old synccrate:// links still work.
- **CurseForge for The Sims 4, Minecraft and WoW.** Check for updates now recognises mods, CC and addons from CurseForge: real names, creators, pictures, newer versions and a *What's new* link. For Minecraft it also warns about mods that need another mod you don't have, or that were made for a different Minecraft version or loader than the rest of your mods. It only runs when you click, and sends file fingerprints, never names or files.
- **Share as a link.** Some creators ask people to download their mods from them (Patreon early access, "no reuploads"). Mark a mod or a creator folder as a link in its details: friends get the creator's download link instead of the file, and packs carry those links too.
- **Hosting or joining?** New users start with that question and are hosting or joining a friend within three screens. You can also paste a whole invite link into the Join box.
- **Discord status (optional).** Show "Hosting The Sims 4 · 2 friends connected" on your Discord profile, never your join code or friends' names. Off until you turn it on; SyncCrate asks once, the first time you host.
- **WoW Forever** is supported as its own game. Its install folder will be confirmed when it launches on 4 November; until then you may need to point SyncCrate at it once.

### Smaller improvements
- **Max upload speed** is now one limit shared by everyone syncing from you (it used to apply per friend) and changes take effect right away.
- **Copy diagnostics** (Network Check, Settings, and on failed connections) copies a report for bug reports with codes, PINs, addresses and known names removed. Nothing is sent anywhere.
- Faster link checks on huge Mods folders, Sims 4 CC matched more often on CurseForge, and many smaller fixes from two code reviews.

> **Tip:** When a creator asks for downloads from their page, use **Share as a link** instead of sharing the file. Friends still see exactly what they're missing.

## 0.8.0 — 2026-10-08

Your mods get faces. The Content page now shows real pictures and names for Sims 4 and Sims 3 CC and for mods in six more games, and a new Grid view lets you browse your whole Mods folder by picture.

### Highlights
- **Sims 4 CC with thumbnails and real names.** Every `.package` shows the thumbnail stored inside it. Build/buy objects get their catalog name (so `icemunmun_CinnamonRolls.package` becomes "Cinnamon Roll"), CAS items get a cleaned-up name, and the `[Creator]` tag in the file name becomes the author. Merged packages show how many items they hold. Click an item for a big preview.
- **Grid view.** Switch the Content page to **Grid** to browse mods and CC as picture tiles. It stays smooth with tens of thousands of files.
- **Real names and pictures for more games:** RimWorld, Project Zomboid, Factorio, Darkest Dungeon, XCOM 2 and World of Warcraft addons, on top of Thunderstore, SMAPI, Minecraft, Paradox and Bannerlord mods.
- **Sims 3 CC thumbnails.** Sims 3 packages show the thumbnail or icon stored inside them too.
- **A new-look Windows installer** to match the app.

### Smaller improvements
- The Sims 4 Mods tab is now called **Mods & CC**, since most of it is CC.
- Clearer wording throughout: "friends" instead of "peers", plain "Checking your files…" instead of "Hashing", proper plurals, and messages that say what to do next.
- A "port already in use" error now tells you how to fix it.
- The mod details window scrolls instead of running off small screens.
- Minecraft, Stardew and other mods are no longer called "script mods".
- macOS: the app is now signed ad hoc, so macOS should offer **Open Anyway** instead of calling the app damaged.

> **Note:** Sims 3 thumbnails and the six new games' mod info were built from the file formats' documentation and tested with sample files, not yet with real players' mods. If something shows the wrong name or no picture, please [open an issue](https://github.com/stixez/SyncCrate/issues).

## 0.7.5 — 2026-10-07

23 more games, from Elden Ring's Seamless Co-op to Football Manager 26 graphics. SyncCrate now supports 123 games.

### New games
- **Co-op that needs matching mods:** Elden Ring and Dark Souls III (Seamless Co-op, including the co-op password), Payday 2 (SuperBLT mods, mod_overrides and custom heists), Divinity: Original Sin 2, Peak, Content Warning, Ready or Not and Monster Hunter Wilds.
- **Multiplayer strategy:** Civilization VII, Europa Universalis V, Crusader Kings II and Heroes of Might and Magic III (Horn of the Abyss, GOG Complete edition).
- **Big mod setups to move or share:** Oblivion Remastered, Fallout 3, Hogwarts Legacy, Kingdom Come: Deliverance, Hollow Knight: Silksong, Cult of the Lamb and The Long Dark.
- **Classic co-op and custom content:** Neverwinter Nights: Enhanced Edition, Grim Dawn and Titan Quest Anniversary Edition.
- **Football Manager 26:** facepacks, logos, kits and editor data.

The game's own files and paid DLC are never copied: Oblivion Remastered's Deluxe content and Fallout 3's DLC stay where they are.

### Fixes
- Turning a mod off in Palworld and Civilization VI now really turns it off. Those games also load mods from subfolders, so SyncCrate now renames a disabled mod instead of moving it.

The new games are set up from each game's documented folders. If one is different on your PC, point SyncCrate at the right folder once, and [tell us on GitHub](https://github.com/stixez/SyncCrate/issues) so we can fix it for everyone.

> **Tip:** every game has a guide at [synccrate.app/games](https://synccrate.app/games/) with the folders SyncCrate syncs and the common problems.

## 0.7.1 — 2026-10-07

Take turns on a save with your crew, sync saves that live outside the game folder (Valheim, Stardew Valley, RimWorld), and catch more broken Sims 4 mods, plus a round of connection fixes.

### Highlights
- **Shared saves: take turns on one save.** Share a save with your crew from the Dashboard's new "Shared saves" panel and it only moves when someone hands it over. A friend takes the host's copy, plays, and gives it back. Everyone sees who has the newest copy and who is playing, and a save someone is playing can't be taken. The game has to be closed on both sides, and a save only changes hands once the copy has fully arrived and matches. Everyone needs to be in the same crew and connect from **Crews** or with the join code; over the local network or by IP you'll get a message explaining why it isn't available.
- **Saves outside the game folder now sync.** Valheim worlds, Stardew Valley farms and RimWorld saves live in your user folders, not next to the mods, so they never synced before. SyncCrate now finds them automatically, and you can pick a different folder in Settings. A friend who doesn't have the folder yet still gets the mods.
- **More Sims 4 mod health checks, with one-click fixes.** The Content page now also catches:
  - two copies of the same script mod (even renamed), with "Turn off older copies"
  - a missing or empty `Resource.cfg`, which it writes for you
  - Tray items saved into Mods, and CC saved into Tray, with a button to move them
  - script mods named in the game's own error reports since the last game update

  Fixes wait until the game is closed and never overwrite your files.

### Fixes
- A connection that dropped silently no longer leaves Sync or Cancel stuck for up to two minutes, and a host that went away is noticed much sooner.
- A friend who reconnected several times after a network blip is no longer turned away without a reason.
- The mod-size check no longer reports "too deep" when you've added deeper folder lines to `Resource.cfg` yourself.
- The Linux AppImage now has a proper app icon for AppImage launchers and installers.

### Good to know
- **Update every PC in your crew.** Shared saves and outside-folder saves need 0.7.1 on both sides. An older version still syncs mods normally.
- **macOS 11 (Big Sur) or newer is now required.**
- These features were checked in the demo and by automated tests. Shared saves are new, so keep a backup of a world you care about while trying them out.

## 0.7.0 — 2026-09-26

A big one: SyncCrate can now find the mod that's breaking your game, start the game for you, tell the host when each friend is done, and it explains itself with a built-in guide. Plus a long list of safety fixes — **updating is recommended on every PC**, and friends on 0.6.0 can still sync with you while they update.

### Highlights
- **Find a broken mod.** The 50/50 method, done for you: SyncCrate turns half of your mods off, you check the game and answer "Still broken" or "It's fixed now", and it halves again until one mod (or creator folder) is left — about a dozen rounds even for thousands of mods. Your progress is saved between rounds, your own disabled mods are left alone, and at the end everything goes back on (or you keep the broken one off). On the Content page.
- **Play.** A Play button on the Dashboard starts the game through Steam, or The Sims 4 on EA app installs.
- **See what's new, and who's done.** After a sync the Dashboard lists what arrived ("From Alex: 12 new mods…"). Hosts now see when each friend finished syncing ("synced 42 files, 2 failed"), get told when a friend drops out mid-download or offers files, and see an amber badge when a friend is on a different game patch.
- **Guide.** A new Guide page in the sidebar explains every feature one task at a time, with search and a button that takes you to the right page.
- **Stay in sync, for real.** It now keeps pulling the host's new mods in the background on any page, says when it last checked, and doesn't pop up a message every minute.
- **Nicer everyday use.** Your name is asked on first run, the window remembers its size and place, the host's PIN stays the same between sessions (so saved join codes keep working; **New PIN** retires it), "Always skip" a file for a game right from the sync list, search the sync list, apply a saved profile, copy your join code from the tray, pin games in the sidebar, and a much better Activity page (search, filters, copy or save the log, sync history with speed and errors).

### Safety fixes
- **Paid content no longer syncs** for Skyrim, Fallout 4 / New Vegas, Oblivion, Morrowind, Starfield, Kerbal Space Program and Mount & Blade: only mods sync, never the game's own files, DLC or Creation Club content. The game list got a big audit (52 games fixed: folders, file types, what's excluded).
- Settings, undo records, file history and crews can no longer be wiped by a crash, a power cut, a damaged file or a file held by antivirus/OneDrive.
- Synced files keep the host's file dates (games that order mods by date, like Oblivion and New Vegas, load them in the same order).
- Deleting a file you changed after Compare, undo after a resumed sync, undo without a "back up before sync", and restoring a single file from history all got fixed.
- A 50/50 search, pack revert or toggling in a junctioned or renamed game folder can always turn mods back on.
- Sims 4 pack detection and the "missing packs" warning now work for normal installs (they only matched by accident before, and often warned about packs you own).

### Smaller improvements
- Faster startup, and big libraries (30,000+ files) stay responsive: fewer rescans, bulk enable/disable in one step, long lists shown in pages.
- Settings → Storage shows how much space SyncCrate uses per game and can clear a game's file history.
- Keyboard and screen-reader improvements across the app; clearer error messages.
- Many small fixes: conflicts are easier to find and resolve, disconnect asks first while friends are downloading, "Show in folder" selects the file, and more.

> **Tip:** the Guide page (sidebar) is the quickest way to see everything SyncCrate can do.

## 0.6.0 — 2026-09-26

The biggest update so far: save your friend group once and catch up in one click, chat while you sync, see what your mods actually are (and whether they have updates), and undo any file a sync changed. Plus a security pass and a lot of fixes for very large mod folders.

### Highlights
- **Crews.** Save your group ("Sunday Sims Crew") with its members and games. Anyone in the crew can host, and a friend who missed a session catches up in one click. Crew invites are links you can send in chat.
- **Session chat.** Talk to everyone in the session right next to the sync, with "Alex joined" and "Sam finished syncing 42 files" lines in the same feed. Works on LAN and over the internet.
- **Mod names, icons and update checks.** The Content page shows real mod names, authors and icons where the mod includes them (BepInEx/Thunderstore, SMAPI, Minecraft Fabric/Quilt/Forge, Paradox, Bannerlord), and can check Modrinth, Thunderstore and SMAPI for newer versions.
- **File history.** When a sync replaces or deletes a file, the old version is kept. Put back just that one file from the mod's details or Backups → File history. Deleting duplicates and overwriting a mod on install are kept too.
- **Stay in sync.** Turn it on and new mods from the host arrive automatically while you're connected. Scripts and anything that would replace your own files still wait for you.
- **Friends can offer mods to the host.** Found a mod the group should have? Offer it; the host picks what to accept, and it's checked again when it arrives.
- **Mod health check.** Warns about common "same files but it still crashes" problems, starting with The Sims 4 (script mods turned off in the game, CC left inside .zip files, missing loaders).

### Safety and security
- **PIN is on by default** for new sessions, with lockouts against guessing.
- Files from friends are checked more strictly: no program files, no Windows device names, nothing outside the game's mod folders.
- A join that was still connecting when you stopped hosting can no longer slip in afterwards.
- Hosting, installing and turning mods on or off are paused while a backup is being restored, so nobody pulls a half-restored folder.

**Updating is recommended for everyone in your group.** 0.6.0 still connects to 0.5.6, but crews, chat, offers and the new safety checks need both PCs on 0.6.0.

### Fixes and smaller improvements
- Very big mod folders (35,000+ files) can now host and join. They used to be dropped right after connecting.
- Syncs with tens of thousands of files are much faster, and the app no longer rescans everything twice a second while files are copied.
- 24 games (including Stardew Valley, Valheim, RimWorld, Skyrim SE, Palworld and Bannerlord) are now found in every Steam library, not just the default two folders.
- One very large changed file no longer makes every file after it fail.
- Entering a 5-digit PIN in the "this host needs a PIN" prompt works again.
- Clicking Compare & Sync right after connecting no longer fails with "no file list yet".
- "Apply pack exactly" no longer says "applied" when a conflict kept your own version.
- An interrupted undo can be retried, and scheduled backups that keep failing now tell you.
- Friendlier error messages ("a file is in use by the game", "your drive is full"), one toast per sync instead of two, a "Reconnecting…" banner when the host drops, and clearer empty screens.
- 7 Days to Die: the Saves entry was removed. Its saves live outside the game folder and never synced; Mods and Server Configs still do.

> **Tip:** Make a crew with the friends you play with most. Next time, whoever is online can host and everyone else just clicks Join.

## 0.5.6 — 2026-09-24

A safety release. It fixes a bug that could sync one game's mods into another game's folder, makes "Detected" games accurate, closes a series of edge cases where files could be overwritten without asking, and rebuilds backups so they're fast, small and actually run on schedule. **Updating is recommended on every PC** — the wrong-game protection works once both the host and the friend are on 0.5.6.

### Highlights
- **Never syncs into the wrong game.** If a friend joins with a different game selected (say Euro Truck Simulator 2 while you host The Sims 4), SyncCrate refuses and offers a one-click **Switch to The Sims 4 and join**. *Scan for Hosts* now shows which game each host is sharing. Files that don't belong to your game's folders are never downloaded, even from older hosts.
- **"Detected" means installed.** Games are only marked Detected when they're really installed (Steam, EA app, Battle.net, Epic, GOG and more). Leftover mod folders from uninstalled games now show as "Folder found" instead.
- **Nothing gets overwritten without your say.** A sync never replaces a different file on your PC unless you chose "Use theirs", folders whose names only differ in capital letters are treated as a conflict, and if the host changes a file after you compared, that file is skipped instead of silently swapped.
- **Backups, rebuilt.** Backups are now incremental: unchanged files are stored once, so repeat backups take seconds and barely use disk space. Scheduled backups actually run (every game in your library, remembered across restarts). The backup before a sync only saves the files that sync will replace, and the sync stops if that backup fails. Restores keep original file dates, show a summary, can optionally remove files added since the backup, and won't run while the game is open.

### Fixes
- Changing a game's folder after comparing no longer lets the old sync plan write into the new folder.
- Mods you disabled stay disabled instead of being re-downloaded enabled; "Keep both" picks a free name and never overwrites an earlier copy.
- The Content page, profiles and the duplicate finder can no longer act on a different game than the one you're looking at; during a session the page is read-only.
- The duplicate finder is only offered for games where duplicate mods are a real problem (The Sims, truck and farming sims), only looks at mods, and re-checks files right before deleting.
- Game folders on a disconnected drive are kept instead of being replaced by a leftover folder; picking a drive root, your Documents folder or another game's folder is refused, and unusual folders ask for confirmation.
- Drag & drop puts files into the right folder for their type (it failed or used the wrong folder for many games).
- Disabling mods in BepInEx games (Valheim, Lethal Company, R.E.P.O., …) now really stops them loading. Stardew Valley and KSP hide the toggle because it can't work reliably there.
- Backup lists show real counts per folder (not "0 mods" for WoW and others), empty backups are never created or used to prune good ones, and restores can't run during a sync or session.
- Mod tags are kept per game and follow a mod when you disable it.

> **Tip:** if you synced while the wrong game was selected before this update, check that game's folder for files that don't belong there.

## 0.5.5 — 2026-09-23

A brand-new look. SyncCrate has been redesigned from the ground up to feel like a proper game launcher, with real game art, your own colors and a content browser that copes with huge mod folders.

### Highlights
- **Complete redesign.** A sharp, game-launcher style interface across every screen, with a new logo, app icon and mint color scheme.
- **Real game art.** Official box art and backgrounds for all 100 games: from Steam, or from the publisher for games like World of Warcraft, Warcraft III, Minecraft and Vintage Story. Art is downloaded the first time you open a game and kept on your PC, so it works offline afterwards. You can set your own cover for any game, or turn art off in Settings.
- **Make it yours.** A new **Appearance** section: pick an accent color (8 presets or any custom color, with a live preview), or let each game use its own color. Choose dark, light or follow Windows, scale the interface from 90% to 125%, switch to compact rows, and turn off visual effects for a calmer look (off automatically if Windows is set to reduce motion).
- **A game browser that finds things.** Filter all 100 games by genre, by installed/in library/not added, sort by name or series, and switch between a grid of covers and a compact list. Search also matches genres, and `/` or `Ctrl+F` jumps to it.
- **Content page built for big mod folders.** Browse mods by folder and enable or disable a whole creator's folder in one click, filter by enabled, disabled, outdated, conflicts, file type and tag, and see totals at a glance. The list scrolls smoothly even with 20,000+ files.

### Smaller improvements
- Each mod row has its own on/off switch and a "Modified" column; file hashes moved into the details panel.
- Detected games are pre-selected on the welcome screen again.
- Clicking the tag editor no longer opens the mod details, Enter in the backup name field creates the backup, and "Save profile" waits for a name.

> **Tip:** anyone with your join code can join, so turn on **Use PIN** before sharing a code publicly.

## 0.5.0 — 2026-09-23

The biggest update yet: friends can now join from anywhere with a single code, connections are much more reliable, and SyncCrate supports **100 games**.

### Highlights
- **Join from anywhere.** The host shares one join code and friends paste it, on the same Wi-Fi or across the internet. No VPN, no port forwarding. Connections are peer-to-peer and encrypted, powered by [iroh](https://iroh.computer), with an encrypted relay as a fallback. Files are never stored on a server.
- **Connections that just work.** SyncCrate finds hosts via mDNS *and* UDP broadcast, tries every network address a host has, and gives clear error messages. A one-click **Fix Windows Firewall** removes the rules that silently blocked connections. No need to run as administrator.
- **100 games** (up from 46), including Baldur's Gate 3, Cyberpunk 2077, Fallout 4, GTA V, Stellaris, Lethal Company and Balatro. Steam games are now found on every drive.
- **Sims 4, done right:** ReShade *and* GShade presets and shaders, correct mod disabling (the game loads subfolders, so disabled mods now stay disabled), automatic thumbnail-cache clearing after sync, a "may be outdated" flag for script mods older than the last patch, and a one-click fix for mods stuck in the old `_Disabled` folder.

### Quality of life
- Duplicate mod finder, "Keep newer" for all conflicts, and the ability to cancel a running sync (it resumes later).
- Size and time estimate before syncing, a "Host added N files" badge, and desktop notifications.
- Warns when the game is still running during a sync.
- Remembers your name and hosting settings, asks for the PIN when a host needs one, and offers one-click **Reconnect**.
- Tray status with Stop hosting / Disconnect, plus optional close-to-tray.
- Network Check panel with firewall status and a "can I reach this host?" test.

### Fixes
Around 30 bugs fixed, including: host folder permissions not being applied, "Keep both" conflicts always failing, false conflicts after quick scans, random disconnects from partially read messages, "Stop hosting" hanging, leftover `.tmp` files, backups copying whole folders, restores overwriting files when the safety backup failed, and the file watcher not following game switches.

> **Tip:** anyone with your join code can join, so turn on **Use PIN** before sharing a code publicly.

## 0.4.8 — 2026-06-01

### 🔌 Connection stability
- Fixed the **"Sync failed – no active connections"** error when joining a host with a large mod folder (most common with big Sims 4 Mods directories). The host no longer re-hashes its entire content folder during the handshake under a too-short timeout, so first-time connections succeed reliably.
- Eliminated a redundant full re-hash of every file on each transfer, which could make large files time out mid-sync.
- The join screen now shows an accurate **"Connecting…"** state and only reports *connected* once the handshake actually completes — no more syncing into a connection that isn't ready yet.

### 🔄 Clearer sync model
- Sync is now explicitly **pull-based**: the host shares files and joining players download them. Removed misleading "upload" actions that never actually transferred, and the host no longer shows a non-functional **Compare & Sync** button.

### 🎮 The Sims 4 — ReShade syncing
- New **"The Sims 4 (ReShade)"** entry syncs your **ReShade presets and shaders** straight from the game's Bin folder. Presets go in `reshade-presets\` (auto-created), shaders in `reshade-shaders\`. Your machine-specific `ReShade.ini` and the ReShade DLL are never synced. (Both players still need ReShade itself installed.)

## 0.4.7 — 2026-04-17

- Fixed a client session race that could disconnect peers 5-10 seconds after connecting.
- Added support for Project Zomboid, Skyrim Special Edition, and Mount & Blade II: Bannerlord.
- Updated the supported games count to 46 games across 30 families in the README and landing page.

## 0.4.6 — 2026-03-28

### New Features
- **Bandwidth throttling** — Limit transfer speed (1-100 MB/s or unlimited) in Settings to avoid saturating your network. Great for Tailscale/ZeroTier users on limited connections.
- **Sync history** — New "Sync History" tab in Activity page. Tracks your last 100 syncs with peer name, files transferred, data size, and direction. Persists across app restarts.
- **Sync plan pagination** — Sync plan details now load 50 items at a time with "Show more" instead of rendering everything at once. Fixes lag with large mod folders.

### Bug Fixes
- **Fixed sync plan showing all files as needing sync** — When connecting to a peer, the app was sending file manifests without SHA-256 hashes (from a quick scan), making every file appear different. Now both host and client automatically re-scan with full hashes before exchanging manifests.

### Under the Hood
- Extracted `scan_files_inner` for reuse between Tauri commands and the transfer layer
- 35 unit tests (6 new: config serialization, sync history entries, checkpoint roundtrip)
- Code review fixes: removed throttle drift bug, eliminated per-file config reads, deduplicated `formatBytes`

## 0.4.5 — 2026-03-27

### Transfer Compression
- File transfers are now compressed with **zstd**, reducing bandwidth by 50-80% for most mod types
- Compression is negotiated automatically between peers — backward-compatible with older versions
- Already-compressed files (.zip, .png, .mp3, etc.) and files under 1KB are sent uncompressed
- Per-chunk fallback: if compression inflates a chunk, it's sent raw instead

### Scheduled Auto-Backups
- New **"Back up before sync"** toggle — automatically creates a backup before every sync
- New **"Scheduled backups"** toggle with configurable interval (1h, 2h, 4h, 8h, 12h, 24h)
- **Auto-pruning** — set a max number of auto-backups (1-20) and old ones are cleaned up automatically
- Auto-backups are labeled with timestamps and marked with an **"Auto" badge** on the Backups page
- Manual backups are never pruned
- Configure everything in **Settings → Auto-Backups**

### Sync Transfer Resume
- If a sync is interrupted (network drop, app close), reconnecting **resumes from where it left off**
- Progress is checkpointed to disk after each file completes
- If files changed since the interruption, the checkpoint is discarded and sync starts fresh (safe default)
- Resume status shown on the Dashboard: "Resuming — X files already transferred"

### 3 New Games (43 total, 27 families)
- **Subnautica** — QMods + saves
- **7 Days to Die** — Mods, saves, server configs
- **Kerbal Space Program** — GameData mods, saves, ship designs

### Testing
- Added 29 unit tests covering compression, sync diffing, security utils, and glob matching

## 0.4.4 — 2026-03-19

### 6 New Games (34 → 40)
- **RimWorld** — Mods & Save files
- **Cities: Skylines** — Mods, Assets, Saves & Maps
- **Euro Truck Simulator 2** — Mods & Profiles
- **Factorio** — Mods, Saves, Scenarios & Blueprints
- **Palworld** — Mods (LogicMods)
- **Left 4 Dead 2** — Addons/Maps & Configs

### Host IP Display
- Hosts now see their **local IP addresses** directly in the dashboard — no more switching to Tailscale/ZeroTier to find your IP
- Click any IP to copy it to clipboard instantly
- Tailscale and ZeroTier IPs are auto-labeled so you know which one to share

### Improved VPN & Firewall Guidance
- Redesigned connection guide with a prominent **firewall setup section** — the #1 cause of connection failures over Tailscale/ZeroTier
- Includes a **one-click PowerShell command** to add the firewall rule
- Step-by-step troubleshooting checklist for connection issues
- Clearer error messages when connections are blocked by firewall

## 0.4.3 — 2026-03-18

- **Direct IP Connect** — VPN and Tailscale users can now connect by entering the host's IP address directly, bypassing mDNS discovery
- **Smart Auto-Reconnect** — Connection recovery now tries direct IP first, then falls back to network scan. Works reliably over VPN
- **Host-Side Download Progress** — Hosts can now see real-time download progress for each connected peer

### Improvements
- VPN guidance added to connection guide with clear instructions for Tailscale and ZeroTier users
- Hosts now see their session port displayed, making it easy to share connection details with remote peers
- Active sessions are disconnected before installing updates for cleaner process exit

### Bug Fixes
- Fixed disconnect event missing clean flag, which caused the app to repeatedly search for a host named "all" after disconnecting
- Fixed stale cancellation token in TCP listener that could prevent new connections after restarting a session
- Fixed version reporting being stuck on older versions due to Cargo.toml not being updated during releases
- Fixed reconnect loop not being cancelled when user explicitly disconnects

## 0.4.2 — 2026-03-11

### Game Registry Expansion (15 → 34 games)

19 new games added to the data-driven registry:

- **Valheim** — mods, config, plugins
- **Terraria** — worlds, players, mods, resource packs
- **Call of Duty, Call of Duty 2, Call of Duty 4: Modern Warfare** — mods, custom maps
- **Stronghold HD, Stronghold: Crusader HD, Stronghold 2, Stronghold Crusader 2** — custom maps
- **Space Engineers** — mods, saves, blueprints
- **TrackMania Nations Forever, TrackMania United Forever, TrackMania 2: Stadium, Trackmania (2020)** — tracks, replays, skins
- **Satisfactory** — saves, blueprints
- **Don't Starve Together** — mods, saves
- **Conan Exiles** — mods, saves
- **Torchlight II** — mods, saves, shared stash
- **The Riftbreaker** — saves, mods

### Port Availability Check

The Settings page now validates whether the chosen port is available before saving. Real-time visual feedback indicates if the port is free or already in use, preventing hosting failures from port conflicts.

### Documentation

Updated README and docs site with the full 34-game support table.

## 0.4.1 — 2026-03-06

### Automatic Update Checking
- App now silently checks for updates on startup
- Shows a non-intrusive toast notification when an update is available
- One-click "Update Now" button downloads, installs, and relaunches automatically
- Manual update check still available in Settings

### Bug Fixes
- Fixed version bump for release pipeline

## 0.4.0 — 2026-03-06

### Multi-Game Support
- **15 games supported** across 7 families: Sims 2/3/4, WoW (Retail, Classic, Classic Era, WoTLK, TBC, Vanilla, Custom), Warcraft III, Minecraft Java, Counter-Strike 2, Garry's Mod, and Stardew Valley
- Data-driven game registry — all game definitions live in a single JSON config
- Per-game dashboards, content browsers, profiles, and backups
- Auto-detection for 11 games; manual path setup for WoW private servers

### First-Run Welcome Screen
- Auto-detects installed games on your system
- Select which games to add to your library with one click
- Only shows on first launch — existing users skip it automatically

### New Icon & Branding
- SimShare has been rebranded to **SyncCrate**
- New app icon (crate + sync arrows)
- Updated landing page with new messaging and Minecraft screenshots

### Bug Fixes
- **Fixed: .jar and .dll files blocked during sync** — these are legitimate mod formats for Minecraft and Stardew Valley. Removed from global blocklist; per-game warnings still flag them for review
- **Fixed: Game paths not detected on some platforms** — added environment variable expansion (%APPDATA%, ~) for Minecraft, CS2, GMod, and Stardew paths
- **Fixed: WoW private server paths not persisting** — manually configured paths were being overwritten on app restart
- **Fixed: Backend activeGame desync on startup** — the Rust backend now stays in sync with the frontend's selected game
- **Fixed: Welcome screen flash on existing installs** — initialization now completes before rendering

### Profiles & Sharing
- Export mod profiles as `.synccrate-profile` files
- Share on Discord — friends drag the file into SyncCrate and they're set
- Backward-compatible import of `.simshare-profile` files

### For Contributors
- GitHub issue templates for bug reports and game requests
- PR template for submitting new game support
- Adding a new game requires only a JSON entry — no code changes

### Migration from SimShare

Upgrading from v0.3.0 is seamless:
- Config automatically migrates from simshare to synccrate
- localStorage keys migrate on first launch
- Old .simshare-profile files still importable
- Auto-updater redirects from the old repo URL
