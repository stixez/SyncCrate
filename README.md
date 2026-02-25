<p align="center">
  <img src="simshare/public/vite.svg" width="80" height="80" alt="SimShare" />
</p>

<h1 align="center">SimShare</h1>

<p align="center">
  <strong>Share Sims 4 mods, CC, and saves with friends — directly over your local network.</strong>
</p>

<p align="center">
  <a href="../../releases/latest"><img src="https://img.shields.io/github/v/release/stixez/SimShare?color=1ea84b&label=download&style=flat-square" alt="Latest Release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-1e2d38?style=flat-square" alt="Platforms" />
  <img src="https://img.shields.io/github/license/stixez/SimShare?color=1e2d38&style=flat-square" alt="License" />
</p>

<p align="center">
  <a href="../../releases/latest">Download</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="https://stixez.github.io/SimShare">Website</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="#quick-start">Quick Start</a>&nbsp;&nbsp;&bull;&nbsp;&nbsp;<a href="#building-from-source">Build from Source</a>
</p>

---

## Overview

SimShare is a free, open-source desktop application for syncing Sims 4 mods, custom content, and save files between players on the same local network. No cloud services, no accounts, no file uploads — files transfer peer-to-peer at full LAN speed.

One player hosts a session, others join. SimShare compares mod folders, surfaces differences and conflicts, and lets each player decide exactly what to sync.

---

## Screenshots

<p align="center">
  <img src="docs/screenshots/dashboard.png" width="100%" alt="Dashboard">
</p>
<p align="center"><em>Dashboard — Session overview, sync plan, and connected peers</em></p>

<details>
<summary><strong>More screenshots</strong></summary>

<br>

| Mods & CC | Save Files |
|:-:|:-:|
| ![Mods & CC](docs/screenshots/mods-cc.png) | ![Save Files](docs/screenshots/saves.png) |
| *Browse mods, resolve conflicts* | *Manage and sync save files* |

| Profiles | Activity Log |
|:-:|:-:|
| ![Profiles](docs/screenshots/profiles.png) | ![Activity Log](docs/screenshots/activity-log.png) |
| *Snapshot and share mod setups* | *Real-time session events* |

</details>

---

## Features

| | Feature | Description |
|-|---------|-------------|
| **P2P** | Peer-to-peer transfer | Files move directly between computers. Nothing leaves your network. |
| **Multi** | Multi-peer sessions | One host, multiple clients. Each client syncs independently. |
| **mDNS** | Auto-discovery | Finds peers on your network automatically. No IPs to configure. |
| **Diff** | Smart diffing | Compares file hashes. Only transfers what's actually different. |
| **Resolve** | Conflict resolution | Keep yours, use theirs, or keep both — per file. |
| **Profiles** | Mod profiles | Snapshot your mod setup. Export/import as `.simshare-profile` files. |
| **SHA-256** | Integrity verification | Every file is hash-verified after transfer. |
| **Live** | Real-time progress | File counts, byte totals, and percentage during sync. |

---

## Download

Get the latest release for your platform from the **[Releases](../../releases/latest)** page.

| Platform | Format |
|----------|--------|
| Windows | `.exe` installer |
| macOS (Apple Silicon) | `.dmg` |
| macOS (Intel) | `.dmg` |
| Linux | `.AppImage` / `.deb` |

> On macOS you may need to right-click > **Open** on first launch (the app is not notarized).

---

## Quick Start

### 1. Install

Download and run SimShare on each computer. The app auto-detects your Sims 4 folder on launch.

### 2. Connect

Both players must be on the **same local network** (same Wi-Fi / router).

| Role | Action |
|------|--------|
| **Host** | Enter a display name > **Start Hosting** |
| **Client** | Enter a display name > **Scan for Hosts** > click the host to connect |

### 3. Compare & Sync

Click **Compare & Sync** on the Dashboard. SimShare scans both mod folders and categorizes every file:

- **Download** — files the peer has that you don't
- **Upload** — files you have that the peer doesn't
- **Conflict** — same file exists on both sides with different contents

Resolve any conflicts in the **Mods & CC** or **Saves** tab, then click **Sync Now**.

### 4. Done

Click **Disconnect** when finished. All transferred files are already saved.

---

## Remote Players

Not on the same network? SimShare works over any virtual LAN. We recommend **[Tailscale](https://tailscale.com)** (free for personal use):

1. Both players install Tailscale and join the same network
2. Use SimShare normally — mDNS discovery works through Tailscale

---

## Mod Profiles

Profiles capture a snapshot of your current mod list.

| Action | How |
|--------|-----|
| Create | Profiles tab > **+** card > name & description |
| Export | Click export on a profile card > saves a `.simshare-profile` file |
| Import | Click **Import** > select a `.simshare-profile` file |
| Delete | Click the trash icon on a profile card |

---

## Building from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) stable
- Platform dependencies per [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

### Build

```bash
git clone https://github.com/stixez/SimShare.git
cd SimShare/simshare
npm install
npm run tauri dev        # development with hot reload
npm run tauri build      # production build with installer
```

Output: `src-tauri/target/release/bundle/`

---

## Architecture

| Layer | Technology |
|-------|-----------|
| Framework | [Tauri v2](https://v2.tauri.app/) |
| Backend | Rust |
| Frontend | React 19 + TypeScript + Vite |
| Styling | Tailwind CSS |
| State | Zustand |
| Networking | TCP (transfer) + mDNS (discovery) |
| Integrity | SHA-256 |

---

## FAQ

<details>
<summary><strong>Is there a file size limit?</strong></summary>
Individual files up to 2 GB. No limit on total sync size.
</details>

<details>
<summary><strong>Can more than two people sync at once?</strong></summary>
Yes. One person hosts, multiple friends join. Each client syncs independently with the host.
</details>

<details>
<summary><strong>Does it sync tray files?</strong></summary>
SimShare syncs <code>.package</code> files (mods/CC) and save files. Tray files are not currently included.
</details>

<details>
<summary><strong>Will it break my mods?</strong></summary>
SimShare never modifies existing files unless you explicitly choose "Use Theirs" on a conflict. It only adds new files or replaces files you approve.
</details>

<details>
<summary><strong>Does it work with pirated copies of The Sims 4?</strong></summary>
SimShare works with any Sims 4 installation that has a standard Mods and Saves folder structure.
</details>

<details>
<summary><strong>Do both players need the same version?</strong></summary>
Yes. Both should run the same version of SimShare for compatibility.
</details>

---

## Contributing

Contributions are welcome. Open an [issue](../../issues) for bugs or feature requests, or submit a pull request.

---

## Support

If SimShare is useful to you, consider [buying me a coffee](https://www.buymeacoffee.com/stixe).

---

## License

[MIT](LICENSE)
