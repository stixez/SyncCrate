import { create } from "zustand";
import type {
  BackupInfo,
  FileManifest,
  GameInfo,
  ModCompatibility,
  ModProfile,
  Page,
  PeerInfo,
  SessionStatus,
  SimsGame,
  SyncPlan,
  SyncProgress,
} from "../lib/types";

interface AppState {
  page: Page;
  setPage: (page: Page) => void;

  gamePaths: Partial<Record<SimsGame, string>>;
  setGamePaths: (paths: Partial<Record<SimsGame, string>>) => void;

  activeGame: SimsGame;
  setActiveGame: (game: SimsGame) => void;

  manifest: FileManifest | null;
  setManifest: (manifest: FileManifest | null) => void;

  session: SessionStatus | null;
  setSession: (session: SessionStatus | null) => void;

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

  isScanning: boolean;
  setIsScanning: (scanning: boolean) => void;

  modTags: Record<string, string[]>;
  setModTags: (tags: Record<string, string[]>) => void;

  isDragging: boolean;
  setIsDragging: (dragging: boolean) => void;

  backups: BackupInfo[];
  setBackups: (backups: BackupInfo[]) => void;

  excludePatterns: string[];
  setExcludePatterns: (patterns: string[]) => void;

  gameInfo: GameInfo | null;
  setGameInfo: (info: GameInfo | null) => void;

  modCompatibility: ModCompatibility[];
  setModCompatibility: (compat: ModCompatibility[]) => void;

  modSearch: string;
  setModSearch: (search: string) => void;
  modFilter: "all" | "mod" | "cc";
  setModFilter: (filter: "all" | "mod" | "cc") => void;
  modTagFilter: string | null;
  setModTagFilter: (tag: string | null) => void;

  theme: "dark" | "light";
  setTheme: (theme: "dark" | "light") => void;
}

export const useAppStore = create<AppState>((set) => ({
  page: "dashboard",
  setPage: (page) => set({ page }),

  gamePaths: {},
  setGamePaths: (paths) => set({ gamePaths: paths }),

  activeGame: "Sims4",
  setActiveGame: (game) => set({ activeGame: game }),

  manifest: null,
  setManifest: (manifest) => set({ manifest }),

  session: null,
  setSession: (session) => set({ session }),

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

  isScanning: false,
  setIsScanning: (scanning) => set({ isScanning: scanning }),

  modTags: {},
  setModTags: (tags) => set({ modTags: tags }),

  isDragging: false,
  setIsDragging: (dragging) => set({ isDragging: dragging }),

  backups: [],
  setBackups: (backups) => set({ backups }),

  excludePatterns: [],
  setExcludePatterns: (patterns) => set({ excludePatterns: patterns }),

  gameInfo: null,
  setGameInfo: (info) => set({ gameInfo: info }),

  modCompatibility: [],
  setModCompatibility: (compat) => set({ modCompatibility: compat }),

  modSearch: "",
  setModSearch: (search) => set({ modSearch: search }),
  modFilter: "all",
  setModFilter: (filter) => set({ modFilter: filter }),
  modTagFilter: null,
  setModTagFilter: (tag) => set({ modTagFilter: tag }),

  theme: (localStorage.getItem("simshare-theme") as "dark" | "light") || "dark",
  setTheme: (theme) => {
    localStorage.setItem("simshare-theme", theme);
    document.documentElement.classList.toggle("light", theme === "light");
    set({ theme });
  },
}));
