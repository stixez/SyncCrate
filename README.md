<p align="center">
  <img src="simsync/public/vite.svg" width="80" height="80" alt="SimSync logo" />
</p>

<h1 align="center">SimSync</h1>

<p align="center">
  Sync your Sims 4 mods, CC, and saves with friends over LAN — no cloud, no accounts, no uploads.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue" alt="Platforms" />
  <img src="https://img.shields.io/badge/version-0.1.1-purple" alt="Version" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
</p>

---

## What is SimSync?

SimSync is a free, open-source desktop app that lets you and your friends share Sims 4 mods, custom content, and save files directly over your local network. No accounts, no file size limits, no waiting for uploads — just connect and sync.

**How it works:** One person hosts a session, and one or more friends join. SimSync compares mod folders, shows you exactly what's different, and lets you choose what to sync. Files transfer directly between your computers at LAN speed.

---

## Features

- **Multi-peer sync** — One host, multiple clients. Everyone syncs independently at the same time.
- **Peer-to-peer sync** — Files transfer directly between computers. Nothing is uploaded anywhere.
- **Auto-discovery** — SimSync finds other users on your network automatically via mDNS.
- **Smart diffing** — Only syncs files that are actually different. Identical files are skipped.
- **Conflict resolution** — When both sides have different versions of the same file, you choose: keep yours, use theirs, or keep both.
- **Mod profiles** — Save snapshots of your current mod setup. Export and share them as `.simsync-profile` files.
- **File integrity** — Every transferred file is verified with SHA-256 checksums.
- **Real-time progress** — Watch sync progress with file counts and a progress bar.
- **Activity log** — See everything that happens: connections, transfers, errors.
- **Host controls** — Kick individual peers from your session at any time.
- **Cross-platform** — Works on Windows, macOS (Intel & Apple Silicon), and Linux.

---

## Download

Grab the latest release for your platform from the [Releases](../../releases) page:

| Platform | Download |
|----------|----------|
| **Windows** | `.exe` installer |
| **macOS (Apple Silicon)** | `.dmg` (M1/M2/M3/M4) |
| **macOS (Intel)** | `.dmg` (x86_64) |
| **Linux** | `.AppImage` or `.deb` |

> **Note:** On macOS, you may need to right-click and select "Open" the first time, since the app is not notarized.

---

## How to Use

### Step 1: Install & Launch

Download and install SimSync on both computers. Launch the app — it will automatically detect your Sims 4 folder.

> If your Sims 4 folder is in a non-standard location, you can set the path manually in the app.

### Step 2: Connect

You and your friend need to be on the **same local network** (same Wi-Fi, same router, etc.).

**Person A — Host a session:**
1. Type your name in the "Host a Session" box
2. Click **Start Hosting**

**Person B — Join the session:**
1. Type your name in the "Join a Session" box
2. Click **Scan for Hosts**
3. You'll see the host appear — click their name to connect

### Step 3: Compare

Once connected, click **Compare & Sync** on the Dashboard. SimSync will scan both mod folders and show you:

- **Files to download** — Mods your friend has that you don't
- **Files to upload** — Mods you have that your friend doesn't
- **Conflicts** — Files that exist on both sides but are different

### Step 4: Resolve Conflicts (if any)

If there are conflicts, go to the **Mods & CC** or **Saves** tab. For each conflict you'll see three options:

| Option | What it does |
|--------|-------------|
| **Keep Mine** | Ignore their version, keep your file as-is |
| **Use Theirs** | Replace your file with their version |
| **Keep Both** | Download their version with `_remote` added to the filename |

### Step 5: Sync

Once all conflicts are resolved, click **Sync Now**. Files will transfer directly between your computers. You'll see a progress bar and file-by-file updates.

When it's done, you'll see a "Sync complete" message with a count of files transferred.

### Step 6: Disconnect

Click **Disconnect** when you're done. Your files are already saved — no further action needed.

---

## Mod Profiles

Profiles let you save a snapshot of which mods you currently have installed.

- **Create a profile** — Go to the Profiles tab, click the "+" card, give it a name and description
- **Export a profile** — Click the export button on any profile card to save it as a `.simsync-profile` file
- **Import a profile** — Click "Import" and select a `.simsync-profile` file from a friend
- **Delete a profile** — Click the trash icon on the profile card

> Profiles record which mods were in your folder at the time of creation. They're useful for sharing mod lists or keeping track of different setups.

---

## Not on the Same Network?

If you and your friend are in different locations, SimSync still works — you just need a way to connect your networks. We recommend **[Tailscale](https://tailscale.com)** (free for personal use). It creates a virtual private network between your devices, so SimSync sees your friend's computer as if it were on the same Wi-Fi.

1. Both install Tailscale and sign in
2. Both connect to the same Tailscale network
3. Use SimSync normally — it will discover peers through Tailscale

---

## Building from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (latest stable)
- Platform-specific dependencies for [Tauri v2](https://v2.tauri.app/start/prerequisites/)

### Steps

```bash
git clone https://github.com/yourusername/SimSync.git
cd SimSync/simsync
npm install
npm run tauri dev      # Development mode with hot reload
npm run tauri build    # Production build with installer
```

The production build output will be in `src-tauri/target/release/bundle/`.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | [Tauri v2](https://v2.tauri.app/) |
| Backend | Rust |
| Frontend | React 19, TypeScript, Vite |
| Styling | Tailwind CSS |
| State | Zustand |
| Networking | TCP (direct), mDNS (discovery) |
| Hashing | SHA-256 |

---

## FAQ

**Q: Does SimSync work with pirated copies of The Sims 4?**
A: SimSync works with any Sims 4 installation that has a standard Mods and Saves folder structure.

**Q: Is there a file size limit?**
A: Individual files up to 2 GB are supported. There's no limit on total sync size.

**Q: Can more than two people sync at once?**
A: Yes! One person hosts and multiple friends can join simultaneously. Each client syncs independently with the host.

**Q: Does it sync tray files?**
A: SimSync syncs `.package` files (mods/CC) and save files from the Saves folder. Tray files are not currently included.

**Q: Will it break my mods?**
A: SimSync never modifies your existing files unless you explicitly choose "Use Theirs" on a conflict. It only adds new files or replaces files you approve.

**Q: Do both people need the same version?**
A: Yes, both users should be running the same version of SimSync for best compatibility.

---

## Contributing

Contributions are welcome! Feel free to open issues for bugs or feature requests, or submit pull requests.

---

## Support

If you enjoy SimSync, consider supporting development:

- [Buy Me a Coffee](https://www.buymeacoffee.com/stixe)

---

## License

MIT License. See [LICENSE](LICENSE) for details.
