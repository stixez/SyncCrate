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

// --- Per-Game File Manifests ---

export const demoManifests: Record<string, FileManifest> = {
  sims4: {
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
  },

  minecraft_java: {
    files: {
      "mods/sodium-fabric-0.5.8.jar": {
        relative_path: "mods/sodium-fabric-0.5.8.jar",
        size: 1_245_184,
        hash: "mc01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 1 * day,
        file_type: "Mod",
      },
      "mods/lithium-fabric-0.12.1.jar": {
        relative_path: "mods/lithium-fabric-0.12.1.jar",
        size: 892_416,
        hash: "mc02abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 2 * day,
        file_type: "Mod",
      },
      "mods/fabric-api-0.92.0.jar": {
        relative_path: "mods/fabric-api-0.92.0.jar",
        size: 2_621_440,
        hash: "mc03abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 3 * day,
        file_type: "Mod",
      },
      "mods/iris-1.7.0.jar": {
        relative_path: "mods/iris-1.7.0.jar",
        size: 3_145_728,
        hash: "mc04abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 4 * day,
        file_type: "Mod",
      },
      "mods/jei-1.20.4-fabric-17.0.0.jar": {
        relative_path: "mods/jei-1.20.4-fabric-17.0.0.jar",
        size: 1_572_864,
        hash: "mc05abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 5 * day,
        file_type: "Mod",
      },
      "mods/create-fabric-0.5.1f.jar": {
        relative_path: "mods/create-fabric-0.5.1f.jar",
        size: 15_728_640,
        hash: "mc06abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 2 * day,
        file_type: "Mod",
      },
      "mods/xaeros-minimap-24.0.3.jar": {
        relative_path: "mods/xaeros-minimap-24.0.3.jar",
        size: 4_194_304,
        hash: "mc07abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 6 * day,
        file_type: "Mod",
      },
      "mods/modmenu-9.2.0.jar": {
        relative_path: "mods/modmenu-9.2.0.jar",
        size: 512_000,
        hash: "mc08abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 7 * day,
        file_type: "Mod",
      },
      "saves/SMP World/level.dat": {
        relative_path: "saves/SMP World/level.dat",
        size: 8_192,
        hash: "mc09abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 2 * hour,
        file_type: "Save",
      },
      "saves/Creative Build/level.dat": {
        relative_path: "saves/Creative Build/level.dat",
        size: 6_144,
        hash: "mc10abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 3 * day,
        file_type: "Save",
      },
      "resourcepacks/FaithfulPack-1.20.zip": {
        relative_path: "resourcepacks/FaithfulPack-1.20.zip",
        size: 31_457_280,
        hash: "mc11abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 10 * day,
        file_type: "ResourcePack",
      },
      "resourcepacks/VanillaTweaks-CraftingTweak.zip": {
        relative_path: "resourcepacks/VanillaTweaks-CraftingTweak.zip",
        size: 2_097_152,
        hash: "mc12abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 14 * day,
        file_type: "ResourcePack",
      },
      "shaderpacks/ComplementaryShaders_v4.7.2.zip": {
        relative_path: "shaderpacks/ComplementaryShaders_v4.7.2.zip",
        size: 8_388_608,
        hash: "mc13abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 8 * day,
        file_type: "ShaderPack",
      },
      "shaderpacks/BSL_v8.2.09.zip": {
        relative_path: "shaderpacks/BSL_v8.2.09.zip",
        size: 5_242_880,
        hash: "mc14abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcd",
        modified: now - 12 * day,
        file_type: "ShaderPack",
      },
    },
    generated_at: now,
  },

  wow_retail: {
    files: {
      "Interface/AddOns/Details/Details.toc": {
        relative_path: "Interface/AddOns/Details/Details.toc",
        size: 4_096,
        hash: "wow01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 1 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/Details/Details.lua": {
        relative_path: "Interface/AddOns/Details/Details.lua",
        size: 2_097_152,
        hash: "wow02abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 1 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/WeakAuras/WeakAuras.toc": {
        relative_path: "Interface/AddOns/WeakAuras/WeakAuras.toc",
        size: 3_072,
        hash: "wow03abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 2 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/WeakAuras/WeakAuras.lua": {
        relative_path: "Interface/AddOns/WeakAuras/WeakAuras.lua",
        size: 5_242_880,
        hash: "wow04abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 2 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/DBM-Core/DBM-Core.lua": {
        relative_path: "Interface/AddOns/DBM-Core/DBM-Core.lua",
        size: 3_670_016,
        hash: "wow05abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 3 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/ElvUI/ElvUI.toc": {
        relative_path: "Interface/AddOns/ElvUI/ElvUI.toc",
        size: 2_048,
        hash: "wow06abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 4 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/ElvUI/ElvUI.lua": {
        relative_path: "Interface/AddOns/ElvUI/ElvUI.lua",
        size: 8_388_608,
        hash: "wow07abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 4 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/Plater/Plater.lua": {
        relative_path: "Interface/AddOns/Plater/Plater.lua",
        size: 1_572_864,
        hash: "wow08abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 5 * day,
        file_type: "Addon",
      },
      "Interface/AddOns/GTFO/GTFO.lua": {
        relative_path: "Interface/AddOns/GTFO/GTFO.lua",
        size: 409_600,
        hash: "wow09abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 7 * day,
        file_type: "Addon",
      },
      "WTF/Account/PLAYER/SavedVariables/Details.lua": {
        relative_path: "WTF/Account/PLAYER/SavedVariables/Details.lua",
        size: 524_288,
        hash: "wow10abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 1 * hour,
        file_type: "Settings",
      },
      "WTF/Account/PLAYER/SavedVariables/WeakAuras.lua": {
        relative_path: "WTF/Account/PLAYER/SavedVariables/WeakAuras.lua",
        size: 2_097_152,
        hash: "wow11abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 2 * hour,
        file_type: "Settings",
      },
      "WTF/Account/PLAYER/SavedVariables/ElvUI.lua": {
        relative_path: "WTF/Account/PLAYER/SavedVariables/ElvUI.lua",
        size: 1_048_576,
        hash: "wow12abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abcdef01abc",
        modified: now - 3 * hour,
        file_type: "Settings",
      },
    },
    generated_at: now,
  },
};

// Keep backward compat for existing code that uses demoManifest directly
export const demoManifest: FileManifest = demoManifests.sims4;

// --- Session (hosting with PIN) ---

/** Shown instead of the backend's join code in demo mode (no Tauri backend). */
export const demoJoinCode = "SC-8M2K-0QRT-4F7A-J9XC-WB3N-Q2ZD";

export const demoSession: SessionStatus = {
  session_type: "Host",
  name: "GameNight",
  port: 9847,
  peers: [
    {
      id: "peer-1",
      name: "Alex",
      ip: "192.168.1.42",
      port: 9847,
      mod_count: 23,
      version: "0.3.0",
      pin_required: false,
    },
    {
      id: "peer-2",
      name: "Jordan",
      ip: "192.168.1.108",
      port: 9847,
      mod_count: 15,
      version: "0.3.0",
      pin_required: false,
    },
  ],
  is_syncing: false,
  pin: "4829",
  host_ips: ["192.168.1.10", "100.64.0.1"],
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
  excluded: [],
};

// --- Discovered Peers (for join view) ---

export const demoDiscoveredPeers: PeerInfo[] = [
  {
    id: "disc-1",
    name: "GameNight",
    ip: "192.168.1.10",
    port: 9847,
    mod_count: 34,
    version: "0.3.0",
    pin_required: true,
  },
  {
    id: "disc-2",
    name: "ModdedSMP",
    ip: "192.168.1.22",
    port: 9847,
    mod_count: 12,
    version: "0.3.0",
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
    game: "sims4",
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
    game: "sims4",
    mods: [
      { relative_path: "Mods/CC_Hair/aladdin_braids.package", hash: "1a2b3c4d", size: 2_457_600, name: "Aladdin Braids" },
      { relative_path: "Mods/CC_Hair/curly_updo.package", hash: "2b3c4d5e", size: 1_843_200, name: "Curly Updo" },
      { relative_path: "Mods/CC_Clothes/vintage_dress_pack.package", hash: "3c4d5e6f", size: 5_242_880, name: "Vintage Dress Pack" },
      { relative_path: "Mods/CC_Skin/smooth_skin_overlay.package", hash: "6f1a2b3c", size: 921_600, name: "Smooth Skin Overlay" },
    ],
  },
  {
    id: "profile-3",
    name: "Performance Pack",
    description: "Optimized modpack for smooth gameplay with shaders",
    icon: "zap",
    author: "CraftSquad",
    created_at: now - 20 * day,
    game: "minecraft_java",
    mods: [
      { relative_path: "mods/sodium-fabric-0.5.8.jar", hash: "mc01abcd", size: 1_245_184, name: "Sodium" },
      { relative_path: "mods/lithium-fabric-0.12.1.jar", hash: "mc02abcd", size: 892_416, name: "Lithium" },
      { relative_path: "mods/iris-1.7.0.jar", hash: "mc04abcd", size: 3_145_728, name: "Iris Shaders" },
      { relative_path: "mods/fabric-api-0.92.0.jar", hash: "mc03abcd", size: 2_621_440, name: "Fabric API" },
    ],
  },
  {
    id: "profile-4",
    name: "SMP Essentials",
    description: "Must-have mods for our multiplayer server",
    icon: "users",
    author: "CraftSquad",
    created_at: now - 10 * day,
    game: "minecraft_java",
    mods: [
      { relative_path: "mods/fabric-api-0.92.0.jar", hash: "mc03abcd", size: 2_621_440, name: "Fabric API" },
      { relative_path: "mods/create-fabric-0.5.1f.jar", hash: "mc06abcd", size: 15_728_640, name: "Create" },
      { relative_path: "mods/jei-1.20.4-fabric-17.0.0.jar", hash: "mc05abcd", size: 1_572_864, name: "JEI" },
      { relative_path: "mods/xaeros-minimap-24.0.3.jar", hash: "mc07abcd", size: 4_194_304, name: "Xaero's Minimap" },
    ],
  },
  {
    id: "profile-5",
    name: "Raid UI Pack",
    description: "ElvUI + WeakAuras + DBM for raiding",
    icon: "shield",
    author: "GuildMaster",
    created_at: now - 7 * day,
    game: "wow_retail",
    mods: [
      { relative_path: "Interface/AddOns/ElvUI/ElvUI.lua", hash: "wow07abc", size: 8_388_608, name: "ElvUI" },
      { relative_path: "Interface/AddOns/WeakAuras/WeakAuras.lua", hash: "wow04abc", size: 5_242_880, name: "WeakAuras" },
      { relative_path: "Interface/AddOns/DBM-Core/DBM-Core.lua", hash: "wow05abc", size: 3_670_016, name: "Deadly Boss Mods" },
    ],
  },
];

// --- Activity Logs ---

export const demoLogs: LogEntry[] = [
  { id: "1", timestamp: (now - 45 * 60) * 1000, message: "Session started as host \"GameNight\" on port 9847", level: "success" },
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
