import { create } from "zustand";
import type {
  BackupInfo,
  FileManifest,
  ModProfile,
  Page,
  PeerInfo,
  SessionStatus,
  SyncPlan,
  SyncProgress,
} from "../lib/types";

interface AppState {
  page: Page;
  setPage: (page: Page) => void;

  sims4Path: string | null;
  setSims4Path: (path: string) => void;

  manifest: FileManifest | null;
  setManifest: (manifest: FileManifest) => void;

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
}

export const useAppStore = create<AppState>((set) => ({
  page: "dashboard",
  setPage: (page) => set({ page }),

  sims4Path: null,
  setSims4Path: (path) => set({ sims4Path: path }),

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
}));
