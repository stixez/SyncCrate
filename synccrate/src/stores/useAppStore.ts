import { create } from "zustand";
import type {
  ChatLog,
  CrewInvite,
  BackupProgress,
  BackupInfo,
  FileManifest,
  GameDefinition,
  GameInfo,
  ModCompatibility,
  ModProfile,
  Page,
  PeerDownloadProgress,
  PeerInfo,
  SessionStatus,
  SyncPlan,
  SyncProgress,
  UndoStatus,
  ModPack,
  PackApplyPreview,
} from "../lib/types";
import { loadAppearance, loadThemeMode, saveAppearance, saveThemeMode, type Appearance, type ThemeMode } from "../lib/prefs";
import { applyAppearanceRoot, applyThemeClass } from "../lib/appearance";

/** A join/connect attempt with the exact arguments used, for PIN retries. */
export type ConnectAttempt =
  | { kind: "peer"; peerId: string; label: string; pin?: string }
  | { kind: "ip"; ip: string; port: number; name: string; label: string; pin?: string }
  | { kind: "code"; code: string; name: string; label: string; pin?: string }
  | { kind: "crew"; crewId: string; nodeId?: string; name: string; label: string; pin?: string };

interface AppState {
  page: Page;
  setPage: (page: Page) => void;

  // Game registry loaded from backend
  gameRegistry: GameDefinition[];
  setGameRegistry: (registry: GameDefinition[]) => void;

  // User's personal game library (game IDs)
  myLibrary: string[];
  setMyLibrary: (library: string[]) => void;

  // Currently selected game in sidebar (drives Dashboard/Content/Profiles/Backups views)
  selectedGame: string | null;
  setSelectedGame: (game: string | null) => void;

  // Active content type tab within a game's Content page
  activeContentTab: string | null;
  setActiveContentTab: (tab: string | null) => void;

  gamePaths: Record<string, string>;
  setGamePaths: (paths: Record<string, string>) => void;

  // Games with real install evidence (Steam manifest, uninstall entry, ...).
  // A path in gamePaths alone can be a leftover folder, so "Detected" uses this.
  installedGames: string[];
  setInstalledGames: (ids: string[]) => void;

  // Backend active game (used for sync/session context)
  activeGame: string;
  setActiveGame: (game: string) => void;

  manifest: FileManifest | null;
  setManifest: (manifest: FileManifest | null) => void;

  session: SessionStatus | null;
  setSession: (session: SessionStatus | null) => void;

  // True while a join/connect handshake is in flight (before peer-connected fires)
  isConnecting: boolean;
  setIsConnecting: (connecting: boolean) => void;

  // Last connect attempt (exact args) so a PIN rejection can retry it with a PIN
  lastConnectAttempt: ConnectAttempt | null;
  setLastConnectAttempt: (attempt: ConnectAttempt | null) => void;
  // Attempt awaiting a PIN from the user (shown as a PIN prompt on the dashboard)
  pinPrompt: { attempt: ConnectAttempt; wrongPin: boolean } | null;
  setPinPrompt: (prompt: { attempt: ConnectAttempt; wrongPin: boolean } | null) => void;
  /** Set when a host refused us because we had a different game selected. */
  gameSwitchPrompt: { hostGame: string; attempt: ConnectAttempt | null } | null;
  setGameSwitchPrompt: (prompt: { hostGame: string; attempt: ConnectAttempt | null } | null) => void;

  discoveredPeers: PeerInfo[];
  setDiscoveredPeers: (peers: PeerInfo[]) => void;

  syncPlan: SyncPlan | null;
  setSyncPlan: (plan: SyncPlan | null) => void;

  syncProgress: SyncProgress | null;
  setSyncProgress: (progress: SyncProgress | null) => void;

  profiles: ModProfile[];
  setProfiles: (profiles: ModProfile[]) => void;

  showDonate: boolean;
  setShowDonate: (show: boolean) => void;

  donationMilestone: number | null;
  setDonationMilestone: (milestone: number | null) => void;

  isScanning: boolean;
  setIsScanning: (scanning: boolean) => void;

  modTags: Record<string, string[]>;
  setModTags: (tags: Record<string, string[]>) => void;

  isDragging: boolean;
  setIsDragging: (dragging: boolean) => void;

  backups: BackupInfo[];
  setBackups: (backups: BackupInfo[]) => void;
  /** Latest backup-progress / restore-progress event (phase: manual, auto, presync, safety, restore). */
  backupProgress: BackupProgress | null;
  setBackupProgress: (p: BackupProgress | null) => void;

  /** The active game's last sync, if it can still be undone. Client only. */
  undoStatus: UndoStatus | null;
  setUndoStatus: (s: UndoStatus | null) => void;

  /** A `.scpack` file dropped anywhere in the app (see App.tsx's global drop
   * handler); ModpackList picks it up and clears it once loaded. */
  pendingImportPackPath: string | null;
  setPendingImportPackPath: (path: string | null) => void;
  /** An already-validated pack from a clicked link or opened `.scpack` file
   * (`useOpenIntents`); ModpackList compares it and clears it. */
  pendingImportPack: ModPack | null;
  setPendingImportPack: (pack: ModPack | null) => void;
  /** A validated crew invite from a clicked link; CrewList shows "Add crew?". */
  pendingCrewInvite: CrewInvite | null;
  setPendingCrewInvite: (invite: CrewInvite | null) => void;
  /** This session's chat, refreshed on the backend's `chat-updated` event. */
  chat: ChatLog | null;
  setChat: (chat: ChatLog | null) => void;
  /** Bumped on the backend's `crews-changed` event so CrewList reloads. */
  crewsVersion: number;
  bumpCrewsVersion: () => void;
  /** "Apply pack exactly" waiting on its download: the next clean
   * sync-complete runs the disable/re-enable step (`useTauriEvents`). */
  pendingPackApply: { pack: ModPack; preview: PackApplyPreview } | null;
  setPendingPackApply: (p: { pack: ModPack; preview: PackApplyPreview } | null) => void;
  /** Join code from a clicked invite link; the dashboard fills its join box. */
  pendingJoinCode: string | null;
  setPendingJoinCode: (code: string | null) => void;

  excludePatterns: string[];
  setExcludePatterns: (patterns: string[]) => void;

  gameInfo: GameInfo | null;
  setGameInfo: (info: GameInfo | null) => void;

  modCompatibility: ModCompatibility[];
  setModCompatibility: (compat: ModCompatibility[]) => void;

  // Per-peer download progress (host sees peers downloading)
  peerDownloadProgress: Record<string, PeerDownloadProgress>;
  setPeerDownloadProgress: (peerId: string, progress: PeerDownloadProgress | null) => void;
  clearPeerDownloadProgress: () => void;

  modSearch: string;
  setModSearch: (search: string) => void;
  modFilter: "all" | "mod" | "cc";
  setModFilter: (filter: "all" | "mod" | "cc") => void;
  modTagFilter: string | null;
  setModTagFilter: (tag: string | null) => void;

  // Last connected host info (for direct IP reconnect over VPN)
  lastHostIp: string | null;
  lastHostPort: number | null;
  lastHostName: string | null;
  /** Join code of the last host, if we joined with one (works over the internet too). */
  lastHostCode: string | null;
  setLastHost: (ip: string | null, port: number | null, name: string, code?: string | null) => void;
  clearLastHost: () => void;

  // Desktop notifications when the window is in the background (per-user, localStorage)
  notificationsEnabled: boolean;
  setNotificationsEnabled: (enabled: boolean) => void;

  theme: ThemeMode;
  setTheme: (theme: ThemeMode) => void;

  appearance: Appearance;
  setAppearance: (patch: Partial<Appearance>) => void;

  // Compound navigation helpers
  navigateToGame: (gameId: string, page?: Page) => void;
  navigateToGlobal: (page: Page) => void;
}

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Storage unavailable — keep the in-memory value only
  }
}

export const useAppStore = create<AppState>((set, get) => ({
  page: "dashboard",
  setPage: (page) => set({ page }),

  gameRegistry: [],
  setGameRegistry: (registry) => set({ gameRegistry: registry }),

  myLibrary: [],
  setMyLibrary: (library) => set({ myLibrary: library }),

  selectedGame: null,
  setSelectedGame: (game) => set({ selectedGame: game }),

  activeContentTab: null,
  setActiveContentTab: (tab) => set({ activeContentTab: tab }),

  gamePaths: {},
  setGamePaths: (paths) => set({ gamePaths: paths }),

  installedGames: [],
  setInstalledGames: (ids) => set({ installedGames: ids }),

  activeGame: "sims4",
  setActiveGame: (game) => set({ activeGame: game }),

  manifest: null,
  setManifest: (manifest) => set({ manifest }),

  session: null,
  setSession: (session) => set({ session }),

  isConnecting: false,
  setIsConnecting: (connecting) => set({ isConnecting: connecting }),

  lastConnectAttempt: null,
  setLastConnectAttempt: (attempt) => set({ lastConnectAttempt: attempt }),
  pinPrompt: null,
  setPinPrompt: (prompt) => set({ pinPrompt: prompt }),
  gameSwitchPrompt: null,
  setGameSwitchPrompt: (prompt) => set({ gameSwitchPrompt: prompt }),

  discoveredPeers: [],
  setDiscoveredPeers: (peers) => set({ discoveredPeers: peers }),

  syncPlan: null,
  setSyncPlan: (plan) => set({ syncPlan: plan }),

  syncProgress: null,
  setSyncProgress: (progress) => set({ syncProgress: progress }),

  profiles: [],
  setProfiles: (profiles) => set({ profiles }),

  showDonate: false,
  setShowDonate: (show) => set({ showDonate: show }),

  donationMilestone: null,
  setDonationMilestone: (milestone) => set({ donationMilestone: milestone }),

  isScanning: false,
  setIsScanning: (scanning) => set({ isScanning: scanning }),

  modTags: {},
  setModTags: (tags) => set({ modTags: tags }),

  isDragging: false,
  setIsDragging: (dragging) => set({ isDragging: dragging }),

  backups: [],
  setBackups: (backups) => set({ backups }),
  backupProgress: null,
  setBackupProgress: (backupProgress) => set({ backupProgress }),
  undoStatus: null,
  setUndoStatus: (undoStatus) => set({ undoStatus }),
  pendingImportPackPath: null,
  setPendingImportPackPath: (pendingImportPackPath) => set({ pendingImportPackPath }),
  pendingImportPack: null,
  setPendingImportPack: (pendingImportPack) => set({ pendingImportPack }),
  pendingCrewInvite: null,
  setPendingCrewInvite: (pendingCrewInvite) => set({ pendingCrewInvite }),
  chat: null,
  setChat: (chat) => set({ chat }),
  crewsVersion: 0,
  bumpCrewsVersion: () => set((s) => ({ crewsVersion: s.crewsVersion + 1 })),
  pendingPackApply: null,
  setPendingPackApply: (pendingPackApply) => set({ pendingPackApply }),
  pendingJoinCode: null,
  setPendingJoinCode: (pendingJoinCode) => set({ pendingJoinCode }),

  excludePatterns: [],
  setExcludePatterns: (patterns) => set({ excludePatterns: patterns }),

  gameInfo: null,
  setGameInfo: (info) => set({ gameInfo: info }),

  modCompatibility: [],
  setModCompatibility: (compat) => set({ modCompatibility: compat }),

  peerDownloadProgress: {},
  setPeerDownloadProgress: (peerId, progress) =>
    set((state) => {
      const updated = { ...state.peerDownloadProgress };
      if (progress) {
        updated[peerId] = progress;
      } else {
        delete updated[peerId];
      }
      return { peerDownloadProgress: updated };
    }),
  clearPeerDownloadProgress: () => set({ peerDownloadProgress: {} }),

  modSearch: "",
  setModSearch: (search) => set({ modSearch: search }),
  modFilter: "all",
  setModFilter: (filter) => set({ modFilter: filter }),
  modTagFilter: null,
  setModTagFilter: (tag) => set({ modTagFilter: tag }),

  // Persisted in localStorage so "Reconnect to <host>" survives app restarts
  lastHostIp: readStorage("synccrate-last-host-ip"),
  lastHostPort: Number(readStorage("synccrate-last-host-port")) || null,
  lastHostName: readStorage("synccrate-last-host-name"),
  lastHostCode: readStorage("synccrate-last-host-code"),
  setLastHost: (ip, port, name, code = null) => {
    writeStorage("synccrate-last-host-ip", ip);
    writeStorage("synccrate-last-host-port", port ? String(port) : null);
    writeStorage("synccrate-last-host-name", name);
    writeStorage("synccrate-last-host-code", code);
    set({ lastHostIp: ip, lastHostPort: port, lastHostName: name, lastHostCode: code });
  },
  clearLastHost: () => {
    writeStorage("synccrate-last-host-ip", null);
    writeStorage("synccrate-last-host-port", null);
    writeStorage("synccrate-last-host-name", null);
    writeStorage("synccrate-last-host-code", null);
    set({ lastHostIp: null, lastHostPort: null, lastHostName: null, lastHostCode: null });
  },

  notificationsEnabled: readStorage("synccrate-notifications") !== "off",
  setNotificationsEnabled: (enabled) => {
    writeStorage("synccrate-notifications", enabled ? "on" : "off");
    set({ notificationsEnabled: enabled });
  },

  theme: loadThemeMode(),
  setTheme: (theme) => {
    saveThemeMode(theme);
    applyThemeClass(theme);
    set({ theme });
  },

  appearance: loadAppearance(),
  setAppearance: (patch) => {
    const appearance = { ...get().appearance, ...patch };
    saveAppearance(appearance);
    applyAppearanceRoot(appearance);
    set({ appearance });
  },

  navigateToGame: (gameId, page) =>
    set((state) => ({
      selectedGame: gameId,
      page: page ?? "dashboard",
      // Clear stale data when switching to a different game
      ...(state.selectedGame !== gameId
        ? { manifest: null, activeContentTab: null, modCompatibility: [] }
        : {}),
    })),
  navigateToGlobal: (page) => set({ page, selectedGame: null }),
}));
