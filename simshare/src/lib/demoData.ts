import type {
  FileManifest,
  SessionStatus,
  SyncPlan,
  SyncProgress,
  ModProfile,
  LogEntry,
  PeerInfo,
} from "./types";

const now = Math.floor(Date.now() / 1000);
const hour = 3600;
const day = 86400;

// --- File Manifest ---

export const demoManifest: FileManifest = {
  files: {
    "Mods/MC Command Center/mc_cmd_center.ts4script": {
      relative_path: "Mods/MC Command Center/mc_cmd_center.ts4script",
      size: 4_821_504,
      hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
      modified: now - 2 * day,
      file_type: "Mod",
    },
    "Mods/MC Command Center/mc_woohoo.ts4script": {
      relative_path: "Mods/MC Command Center/mc_woohoo.ts4script",
      size: 1_245_184,
      hash: "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3",
      modified: now - 2 * day,
      file_type: "Mod",
    },
    "Mods/WickedWhims/wickedwhims.ts4script": {
      relative_path: "Mods/WickedWhims/wickedwhims.ts4script",
      size: 8_912_896,
      hash: "c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
      modified: now - 5 * day,
      file_type: "Mod",
    },
    "Mods/UIExtensions/ui_cheats.ts4script": {
      relative_path: "Mods/UIExtensions/ui_cheats.ts4script",
      size: 512_000,
      hash: "d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5",
      modified: now - 10 * day,
      file_type: "Mod",
    },
    "Mods/BetterBuildBuy/bbb.ts4script": {
      relative_path: "Mods/BetterBuildBuy/bbb.ts4script",
      size: 2_097_152,
      hash: "e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6",
      modified: now - 3 * day,
      file_type: "Mod",
    },
    "Mods/LittleMsSam/lms_live_in_business.ts4script": {
      relative_path: "Mods/LittleMsSam/lms_live_in_business.ts4script",
      size: 384_000,
      hash: "f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1",
      modified: now - 7 * day,
      file_type: "Mod",
    },
    "Mods/CC_Hair/aladdin_braids.package": {
      relative_path: "Mods/CC_Hair/aladdin_braids.package",
      size: 2_457_600,
      hash: "1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b",
      modified: now - 14 * day,
      file_type: "CustomContent",
    },
    "Mods/CC_Hair/curly_updo.package": {
      relative_path: "Mods/CC_Hair/curly_updo.package",
      size: 1_843_200,
      hash: "2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c",
      modified: now - 12 * day,
      file_type: "CustomContent",
    },
    "Mods/CC_Clothes/vintage_dress_pack.package": {
      relative_path: "Mods/CC_Clothes/vintage_dress_pack.package",
      size: 5_242_880,
      hash: "3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d",
      modified: now - 8 * day,
      file_type: "CustomContent",
    },
    "Mods/CC_Clothes/streetwear_tops.package": {
      relative_path: "Mods/CC_Clothes/streetwear_tops.package",
      size: 3_145_728,
      hash: "4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e",
      modified: now - 6 * day,
      file_type: "CustomContent",
    },
    "Mods/CC_Furniture/modern_kitchen_set.package": {
      relative_path: "Mods/CC_Furniture/modern_kitchen_set.package",
      size: 7_340_032,
      hash: "5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f",
      modified: now - 4 * day,
      file_type: "CustomContent",
    },
    "Mods/CC_Skin/smooth_skin_overlay.package": {
      relative_path: "Mods/CC_Skin/smooth_skin_overlay.package",
      size: 921_600,
      hash: "6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a",
      modified: now - 20 * day,
      file_type: "CustomContent",
    },
    "Saves/Slot_00000001.save": {
      relative_path: "Saves/Slot_00000001.save",
      size: 52_428_800,
      hash: "aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44",
      modified: now - 1 * hour,
      file_type: "Save",
    },
    "Saves/Slot_00000002.save": {
      relative_path: "Saves/Slot_00000002.save",
      size: 48_234_496,
      hash: "bb22cc33dd44ee55ff66aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44ee55",
      modified: now - 1 * day,
      file_type: "Save",
    },
    "Saves/Slot_00000003.save": {
      relative_path: "Saves/Slot_00000003.save",
      size: 31_457_280,
      hash: "cc33dd44ee55ff66aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44ee55ff66",
      modified: now - 4 * day,
      file_type: "Save",
    },
  },
  generated_at: now,
};

// --- Session (hosting with PIN) ---

export const demoSession: SessionStatus = {
  session_type: "Host",
  name: "SimSquad",
  port: 9847,
  peers: [
    {
      id: "peer-1",
      name: "Alex",
      ip: "192.168.1.42",
      port: 9847,
      mod_count: 23,
      version: "0.2.0",
      pin_required: false,
    },
    {
      id: "peer-2",
      name: "Jordan",
      ip: "192.168.1.108",
      port: 9847,
      mod_count: 15,
      version: "0.2.0",
      pin_required: false,
    },
  ],
  is_syncing: false,
  pin: "4829",
};

// --- Sync Plan (with conflicts) ---

export const demoSyncPlan: SyncPlan = {
  actions: [
    {
      SendToRemote: {
        relative_path: "Mods/MC Command Center/mc_cmd_center.ts4script",
        size: 4_821_504,
        hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        modified: now - 2 * day,
        file_type: "Mod",
      },
    },
    {
      ReceiveFromRemote: {
        relative_path: "Mods/TwistedMexi/tool_mod.ts4script",
        size: 1_536_000,
        hash: "7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b",
        modified: now - 1 * day,
        file_type: "Mod",
      },
    },
    {
      ReceiveFromRemote: {
        relative_path: "Mods/CC_Eyes/crystal_eyes.package",
        size: 614_400,
        hash: "8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c0d1e2f7a8b9c",
        modified: now - 3 * day,
        file_type: "CustomContent",
      },
    },
    {
      Conflict: {
        local: {
          relative_path: "Mods/BetterBuildBuy/bbb.ts4script",
          size: 2_097_152,
          hash: "e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6",
          modified: now - 3 * day,
          file_type: "Mod",
        },
        remote: {
          relative_path: "Mods/BetterBuildBuy/bbb.ts4script",
          size: 2_150_400,
          hash: "ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00",
          modified: now - 1 * day,
          file_type: "Mod",
        },
      },
    },
    {
      Conflict: {
        local: {
          relative_path: "Saves/Slot_00000001.save",
          size: 52_428_800,
          hash: "aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44ee55ff66aa11bb22cc33dd44",
          modified: now - 1 * hour,
          file_type: "Save",
        },
        remote: {
          relative_path: "Saves/Slot_00000001.save",
          size: 53_100_544,
          hash: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
          modified: now - 2 * hour,
          file_type: "Save",
        },
      },
    },
  ],
  total_bytes: 61_248_256,
};

// --- Discovered Peers (for join view) ---

export const demoDiscoveredPeers: PeerInfo[] = [
  {
    id: "disc-1",
    name: "SimSquad",
    ip: "192.168.1.10",
    port: 9847,
    mod_count: 34,
    version: "0.2.0",
    pin_required: true,
  },
  {
    id: "disc-2",
    name: "CasPlayground",
    ip: "192.168.1.22",
    port: 9847,
    mod_count: 12,
    version: "0.2.0",
    pin_required: false,
  },
];

// --- Profiles ---

export const demoProfiles: ModProfile[] = [
  {
    id: "profile-1",
    name: "Gameplay Essentials",
    description: "Core gameplay mods for everyday play",
    icon: "gamepad",
    author: "SimSquad",
    created_at: now - 30 * day,
    mods: [
      { relative_path: "Mods/MC Command Center/mc_cmd_center.ts4script", hash: "a1b2c3d4", size: 4_821_504, name: "MC Command Center" },
      { relative_path: "Mods/UIExtensions/ui_cheats.ts4script", hash: "d4e5f6a1", size: 512_000, name: "UI Cheats Extension" },
      { relative_path: "Mods/BetterBuildBuy/bbb.ts4script", hash: "e5f6a1b2", size: 2_097_152, name: "Better BuildBuy" },
    ],
  },
  {
    id: "profile-2",
    name: "CC Lookbook",
    description: "Hair, clothes, and skin CC for photo sessions",
    icon: "sparkles",
    author: "SimSquad",
    created_at: now - 14 * day,
    mods: [
      { relative_path: "Mods/CC_Hair/aladdin_braids.package", hash: "1a2b3c4d", size: 2_457_600, name: "Aladdin Braids" },
      { relative_path: "Mods/CC_Hair/curly_updo.package", hash: "2b3c4d5e", size: 1_843_200, name: "Curly Updo" },
      { relative_path: "Mods/CC_Clothes/vintage_dress_pack.package", hash: "3c4d5e6f", size: 5_242_880, name: "Vintage Dress Pack" },
      { relative_path: "Mods/CC_Skin/smooth_skin_overlay.package", hash: "6f1a2b3c", size: 921_600, name: "Smooth Skin Overlay" },
    ],
  },
];

// --- Activity Logs ---

export const demoLogs: LogEntry[] = [
  { id: "1", timestamp: (now - 45 * 60) * 1000, message: "Session started as host \"SimSquad\" on port 9847", level: "success" },
  { id: "2", timestamp: (now - 40 * 60) * 1000, message: "Peer \"Alex\" connected from 192.168.1.42", level: "info" },
  { id: "3", timestamp: (now - 38 * 60) * 1000, message: "Peer \"Jordan\" connected from 192.168.1.108", level: "info" },
  { id: "4", timestamp: (now - 30 * 60) * 1000, message: "Scanned 15 files (141.2 MB total)", level: "info" },
  { id: "5", timestamp: (now - 28 * 60) * 1000, message: "Sync plan: 5 actions", level: "info" },
  { id: "6", timestamp: (now - 25 * 60) * 1000, message: "Resolved conflict for Mods/BetterBuildBuy/bbb.ts4script: UseTheirs", level: "success" },
  { id: "7", timestamp: (now - 20 * 60) * 1000, message: "Synced 3 files (6.9 MB) with Alex", level: "success" },
  { id: "8", timestamp: (now - 10 * 60) * 1000, message: "Sync plan: 5 actions", level: "info" },
];

// --- Sync Progress (mid-sync snapshot) ---

export const demoSyncProgress: SyncProgress = {
  file: "Mods/CC_Eyes/crystal_eyes.package",
  bytes_sent: 38_400_000,
  bytes_total: 61_248_256,
  files_done: 2,
  files_total: 3,
  peer_id: "peer-1",
};

// --- Apply demo data to stores ---

export function isDemoMode(): boolean {
  if (import.meta.env.PROD) return false;
  return new URLSearchParams(window.location.search).has("demo");
}
