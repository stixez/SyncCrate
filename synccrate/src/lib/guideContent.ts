import type { Page } from "./types";

/**
 * The in-app guide (Guide page). Plain strings with **bold** and `code`;
 * button names match the UI, so rename them here when a button changes.
 */
export interface GuideSection {
  id: string;
  title: string;
  /** One line under the title. */
  summary: string;
  steps: string[];
  tip?: string;
  /** Extra words the search should match. */
  keywords?: string;
  /** "Take me there". Game pages open the active game. */
  go?: { label: string; page: Page };
}

export interface GuideGroup {
  id: string;
  title: string;
  sections: GuideSection[];
}

export const GUIDE: GuideGroup[] = [
  {
    id: "start",
    title: "Start here",
    sections: [
      {
        id: "first-sync",
        title: "Your first sync",
        summary: "Get everyone on the same mods in four steps.",
        steps: [
          "Add your game with **Add Game** in the sidebar. Its folder is found automatically, or set it in **Settings → Games**.",
          "One friend opens the game's Dashboard in SyncCrate and clicks **Start Hosting**.",
          "Everyone else joins: pick the host under **Scan for Hosts** (same network), or paste the host's join code (anywhere).",
          "Click **Compare & Sync**, check the plan, then **Sync Now**.",
        ],
        tip: "Nothing changes on your PC until you confirm the plan.",
        keywords: "host join start begin setup",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
      {
        id: "lan",
        title: "Play on the same network",
        summary: "LAN parties: fast, and no internet needed.",
        steps: [
          "The host clicks **Start Hosting**.",
          "Friends click **Scan for Hosts** and pick the host. SyncCrate tries every address the host has.",
          "Host not showing up? Paste the join code instead, or use **Connect by IP**.",
        ],
        tip: "Run the host on wired Ethernet, click **Fix Windows Firewall** on the host before people arrive, and let friends sync in waves: everyone shares the host's upload.",
        keywords: "lan party local wifi ethernet discovery",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
      {
        id: "internet",
        title: "Play over the internet",
        summary: "Friends somewhere else join with one code. No VPN or port forwarding.",
        steps: [
          "The host shares the `SC-…` join code from their Dashboard, or sends **Copy invite link**.",
          "Friends paste the code or the link into the join box (or click the link: it opens SyncCrate with the code filled in).",
          "SyncCrate connects directly when it can, otherwise through an encrypted relay that can't read your files.",
        ],
        tip: "The code (and its PIN) stays the same between sessions, so friends can use **Reconnect**. Posted it somewhere public? Click **New PIN** while hosting.",
        keywords: "online remote code relay invite link pin",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
    ],
  },
  {
    id: "sync",
    title: "Syncing",
    sections: [
      {
        id: "plan",
        title: "Review what a sync changes",
        summary: "The plan shows every file before anything happens.",
        steps: [
          "After **Compare & Sync**, the plan lists what will be downloaded, sent, deleted, and any conflicts.",
          "Untick a file to leave it out, or add exclude patterns in Settings (`*.ts4script`, `Mods/WickedWhims/*`, or just a file name).",
          "For a conflict, choose: keep yours, use theirs, keep both, or keep newer.",
        ],
        tip: "Cancel any time. A cancelled or dropped sync picks up where it stopped when you compare again.",
        keywords: "exclude conflict cancel resume pattern",
      },
      {
        id: "stay",
        title: "Stay in sync automatically",
        summary: "Get the host's new mods without clicking anything.",
        steps: [
          "While connected, turn on **Stay in sync** on the Dashboard.",
          "Every minute SyncCrate pulls files the host added.",
          "It only ever adds files: anything that would replace or delete one of yours, and script mods, still wait for you to review.",
        ],
        tip: "It pauses while the game is running.",
        keywords: "automatic auto pull background",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
    ],
  },
  {
    id: "safety",
    title: "Safety net",
    sections: [
      {
        id: "undo",
        title: "Undo a sync",
        summary: "Put your files back exactly as they were.",
        steps: [
          "Right after a sync, click **Undo** in the message that appears, or **Undo last sync** on the Dashboard (also on the Backups page).",
          "Replaced files come back identical, and files the sync added are removed.",
          "Anything you changed since the sync is left alone and reported.",
        ],
        tip: "Only the most recent sync per game can be undone.",
        keywords: "revert rollback restore mistake",
        go: { label: "Open Backups", page: "backups" },
      },
      {
        id: "history",
        title: "Get an old version of a file back",
        summary: "File history keeps what a sync replaced or deleted.",
        steps: [
          "Open a mod on the Content page to see its earlier versions (\"replaced by a sync from Alex, Friday 21:40\").",
          "For deleted files, go to **Backups → File history**.",
          "Restore just that one file. The version it replaces is kept too, so you can change your mind.",
        ],
        tip: "Versions are kept for 180 days (up to 10 per file). You can turn file history off in Settings.",
        keywords: "version previous deleted restore file history",
        go: { label: "Open Backups", page: "backups" },
      },
      {
        id: "backups",
        title: "Backups",
        summary: "Full snapshots of your game's folders, stored efficiently.",
        steps: [
          "On the Backups page, click **Create Backup**.",
          "In Settings, turn on scheduled backups (every 1–24 hours) or **Back up before sync**.",
          "Restore any backup. SyncCrate makes a safety backup first and keeps file dates.",
        ],
        tip: "Unchanged files are stored once and shared between backups, so extra backups cost almost nothing.",
        keywords: "snapshot schedule restore safety",
        go: { label: "Open Backups", page: "backups" },
      },
    ],
  },
  {
    id: "share",
    title: "Share and play together",
    sections: [
      {
        id: "modpacks",
        title: "Share a modpack",
        summary: "Send your exact setup as a tiny file.",
        steps: [
          "On the Modpacks page, **Build Pack** exports a `.scpack` list of your files (no file contents).",
          "A friend imports it (double-click, drag and drop, or a link) and sees what's missing or different.",
          "Connected to you, they click **Get Missing Files**. **Apply Pack Exactly** makes their game load only the pack's mods, and **Revert** undoes that.",
        ],
        keywords: "scpack pack export import setup",
        go: { label: "Open Modpacks", page: "modpacks" },
      },
      {
        id: "profiles",
        title: "Save and compare loadouts",
        summary: "Snapshot a setup to check against later.",
        steps: [
          "On the Profiles page, **Create New Profile**, name it and **Save Profile**.",
          "**Compare** shows which mods match, are missing or changed since.",
          "**Apply this loadout** (after Compare) opens it on the Modpacks page: get missing files from a friend, or **Apply Pack Exactly** to load only its mods.",
          "Export a profile as a `.synccrate-profile` file to share it.",
        ],
        keywords: "loadout snapshot profile compare",
        go: { label: "Open Profiles", page: "profiles" },
      },
      {
        id: "crews",
        title: "Crews",
        summary: "Remember your group, its games and its mod set.",
        steps: [
          "On the Crews page, start a crew and send **Copy invite** to your friends.",
          "Whoever hosts can publish their setup as the crew set. Members get it the next time they connect.",
          "The Crews page shows how many files you're behind. **Catch Up** gets you there.",
          "After you've synced with someone once, **Reconnect** reaches them without a code (over the internet).",
        ],
        tip: "Crews live on your PCs only. Being in a crew never gets anyone past the PIN.",
        keywords: "group friends crew set invite",
        go: { label: "Open Crews", page: "crews" },
      },
      {
        id: "handoff",
        title: "Take turns on a save",
        summary: "Hand a shared save around your crew, so nobody plays an old copy.",
        steps: [
          "You need a crew first: on the **Crews** page, under **Start a crew**, click **Create**. Then on the Dashboard, under **Shared saves**, pick a save and click **Share**. From now on it only moves when someone takes it or gives it back, never in a normal sync.",
          "Join your host through the crew (**Crews**, then **Join**). If they have the save and aren't playing, click **Take it**: their copy replaces yours.",
          "Mark **I'm playing** so the others wait. When you're done, click **Give to** your host: your copy replaces theirs.",
          "Two friends hand a save over through whoever hosts: one gives it to the host, the other takes it.",
          "Works for The Sims 3 and 4, Valheim, Minecraft Java, Terraria, Stardew Valley, RimWorld, Factorio, Cities: Skylines and Project Zomboid.",
        ],
        tip: "Close the game before taking or giving a save. The old copy is kept in File history either way.",
        keywords: "save handoff turns rotate legacy world share check out check in",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
      {
        id: "offers",
        title: "Offer a mod to the host",
        summary: "Got a mod the host doesn't? Suggest it.",
        steps: [
          "Select the mod on the Content page and click **Offer to host**.",
          "The host sees the list on their Dashboard and picks what to take.",
          "Everyone else then gets it from the host as usual.",
        ],
        tip: "Nothing is copied until the host accepts, and it can never replace one of the host's files.",
        keywords: "suggest send upload offer",
        go: { label: "Open Content", page: "content" },
      },
      {
        id: "chat",
        title: "Session chat",
        summary: "Talk without Discord open next to it.",
        steps: [
          "Every session has a chat on the Dashboard, on the LAN and over the internet.",
          "Write `@name` to get someone's attention. They get a notification if SyncCrate is in the background.",
        ],
        tip: "Messages only last for the session.",
        keywords: "message talk chat mention",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
    ],
  },
  {
    id: "mods",
    title: "Your mods",
    sections: [
      {
        id: "manage",
        title: "Manage mods",
        summary: "Browse, filter, switch off and tidy up.",
        steps: [
          "The Content page shows your mods by folder, as one list, or as a **Grid** of pictures, with real mod names and icons where mods include them.",
          "Filter by status, type or tag, and enable or disable one mod or a whole creator folder.",
          "Drag files onto the window to install them: each goes to the folder its type belongs in.",
          "Use **Find duplicates** to find identical copies, and **Check for updates** where the game supports it. For Sims 4 and Minecraft it also finds names, pictures and newer versions on CurseForge, sending only file fingerprints (not names or files) through synccrate.app.",
          "Creator doesn't allow re-uploads? Open the mod, paste their page under **Share as a link** and click **Save**. Friends get the link instead of a copy.",
        ],
        tip: "Press `/` or `Ctrl+F` to search.",
        keywords: "content disable enable tag duplicate update install drag drop grid pictures thumbnails link creator reupload patreon early access curseforge",
        go: { label: "Open Content", page: "content" },
      },
      {
        id: "fifty-fifty",
        title: "Find a broken mod",
        summary: "The 50/50 method, without moving folders by hand.",
        steps: [
          "On the Content page, click **Find a broken mod**, then **Start**. Half of your mods are turned off.",
          "Start the game, check whether the problem is still there, close the game, and answer **Still broken** or **It's fixed now**.",
          "SyncCrate halves again until one mod (or creator folder) is left. Even thousands of mods take about a dozen rounds.",
          "Keep the broken one off, or turn everything back on. Only mods the search turned off are turned back on.",
        ],
        tip: "Your progress is saved, so you can close SyncCrate while you play.",
        keywords: "50/50 fifty broken crash bug culprit bisect troubleshoot",
        go: { label: "Open Content", page: "content" },
      },
      {
        id: "sims4",
        title: "The Sims 4 tips",
        summary: "What SyncCrate does differently for Sims 4.",
        steps: [
          "CC shows the thumbnail and name stored inside each `.package`, and the creator from the `[Creator]` tag in its file name. Switch the Content page to **Grid** to browse it by picture; merged packages show how many items they hold.",
          "Disabled mods are renamed to `.disabled`, because the game also loads subfolders. Old `_Disabled` folders are fixed in one click.",
          "The health check on the Content page catches what stops CC from loading: mods switched off in the game's options, script mods more than one folder deep, packages deeper than `Resource.cfg` allows, a missing `Resource.cfg`, two copies of one script mod, and unextracted `.zip` files.",
          "It also finds Tray items saved into Mods and CC saved into Tray (neither works there), and script mods the game's own error report (`lastException`) names. Most have a one-click fix; close the game first.",
          "Outdated script mods are flagged after a game patch, and the thumbnail cache is cleared when new mods arrive.",
          "Saves are personal: a crew set leaves them out unless you tick them.",
          "To share ReShade or GShade presets, add **The Sims 4 (ReShade)** or **(GShade)** as a game.",
        ],
        keywords: "sims sims4 cc custom content script reshade gshade tray health resource.cfg lastexception duplicate",
        go: { label: "Open Content", page: "content" },
      },
    ],
  },
  {
    id: "help",
    title: "Troubleshooting",
    sections: [
      {
        id: "connect",
        title: "Friends can't connect",
        summary: "It's almost always the firewall.",
        steps: [
          "On the **host** PC, click **Fix Windows Firewall** (in the Network Check on the Dashboard).",
          "Use **Test** with the host's IP to see whether it's reachable.",
          "Make sure everyone runs the same SyncCrate version.",
          "Guest Wi-Fi and some mesh routers keep devices apart: use the main network or the join code.",
        ],
        tip: "SyncCrate uses TCP port 9847 for sessions and UDP port 47625 for discovery.",
        keywords: "firewall port network connection failed cant connect",
        go: { label: "Open the Dashboard", page: "dashboard" },
      },
      {
        id: "shortcuts",
        title: "Keyboard shortcuts",
        summary: "Get around faster.",
        steps: ["`Ctrl+Shift+S` opens Settings.", "`/` or `Ctrl+F` searches.", "`Esc` closes a dialog or clears the search."],
        keywords: "keyboard keys hotkeys",
      },
    ],
  },
];

/** Sections matching every word of `query` (title, summary, steps, tip, keywords). */
export function searchGuide(query: string): GuideGroup[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return GUIDE;
  const text = (s: GuideSection) => [s.title, s.summary, s.tip ?? "", s.keywords ?? "", ...s.steps].join(" ").toLowerCase();
  return GUIDE.map((g) => ({ ...g, sections: g.sections.filter((s) => words.every((w) => text(s).includes(w))) })).filter((g) => g.sections.length > 0);
}
