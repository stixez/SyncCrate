import { useEffect, useState, useCallback } from "react";
import { Toaster } from "sonner";
import Layout from "./components/Layout";
import GameDashboard from "./components/GameDashboard";
import ContentBrowser from "./components/ContentBrowser";
import ProfileList from "./components/ProfileList";
import BackupList from "./components/BackupList";
import ModpackList from "./components/ModpackList";
import ActivityLog from "./components/ActivityLog";
import Settings from "./components/Settings";
import GameBrowser from "./components/GameBrowser";
import WelcomeScreen, { isOnboardingComplete } from "./components/WelcomeScreen";
import DropZoneOverlay from "./components/DropZoneOverlay";
import InstallResultsModal from "./components/InstallResultsModal";
import { useAppStore } from "./stores/useAppStore";
import { useLogStore } from "./stores/useLogStore";
import { useTauriEvents } from "./hooks/useTauriEvents";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { setGameRegistry } from "./lib/games";
import {
  isDemoMode,
  demoManifests,
  demoSession,
  demoSyncPlan,
  demoProfiles,
  demoLogs,
  bigSims4Demo,
} from "./lib/demoData";
import type { InstallResult } from "./lib/types";
import { toastSuccess, toastError, toastInfo } from "./lib/toast";
import { gameLabel } from "./lib/games";
import { toast } from "sonner";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import * as cmd from "./lib/commands";

// Migrate localStorage keys from old simshare-* prefix to synccrate-*
function migrateLocalStorage() {
  const migrations: [string, string][] = [
    ["simshare-theme", "synccrate-theme"],
    ["simshare-sync-count", "synccrate-sync-count"],
    ["simshare-donation-dismissed", "synccrate-donation-dismissed"],
    ["simshare-logs", "synccrate-logs"],
  ];
  for (const [oldKey, newKey] of migrations) {
    if (localStorage.getItem(oldKey) !== null && localStorage.getItem(newKey) === null) {
      localStorage.setItem(newKey, localStorage.getItem(oldKey)!);
    }
  }

  // Existing users who already had a library should skip onboarding
  if (localStorage.getItem("synccrate-theme") && !localStorage.getItem("synccrate-onboarding-complete")) {
    localStorage.setItem("synccrate-onboarding-complete", "1");
  }
}

// Run migration once at module load (before any render)
migrateLocalStorage();

/** Rescan `gameId` and show it, unless the user has moved on to another game. */
function refreshIfShown(gameId: string) {
  cmd.scanFiles(gameId)
    .then((m) => { if (useAppStore.getState().selectedGame === gameId) useAppStore.getState().setManifest(m); })
    .catch(() => {});
}

function App() {
  const page = useAppStore((s) => s.page);
  const selectedGame = useAppStore((s) => s.selectedGame);
  const isDragging = useAppStore((s) => s.isDragging);
  const theme = useAppStore((s) => s.theme);
  const addLog = useLogStore((s) => s.addLog);
  useTauriEvents();
  useKeyboardShortcuts();

  const [installResults, setInstallResults] = useState<InstallResult[] | null>(null);
  const [ready, setReady] = useState(false);

  // Startup: load registry + library, detect paths
  useEffect(() => {
    async function init() {
      try {
        // Load game registry from backend
        const registry = await cmd.getGameRegistryCmd();
        setGameRegistry(registry);
        useAppStore.getState().setGameRegistry(registry);

        // Load user library
        const library = await cmd.getUserLibrary();
        useAppStore.getState().setMyLibrary(library);

        // Load all known game paths (saved config + auto-detected)
        const allPaths = await cmd.getAllGamePaths();
        const paths: Record<string, string> = {};
        for (const [id, p] of Object.entries(allPaths)) {
          if (p) paths[id] = p;
        }
        useAppStore.getState().setGamePaths(paths);

        // Install evidence for the "Detected" state; not awaited, the rest of the
        // UI doesn't depend on it.
        cmd.getInstalledGames()
          .then((ids) => useAppStore.getState().setInstalledGames(ids))
          .catch((e) => console.error("Failed to detect installed games:", e));

        // The backend restores the saved active game; mirror it instead of
        // assuming the store default.
        const backendActive = await cmd.getActiveGame();
        useAppStore.getState().setActiveGame(backendActive);

        // Auto-select a game if none selected (skip during onboarding): the saved
        // active game when it's in the library, else the first library game.
        if (!useAppStore.getState().selectedGame && library.length > 0 && isOnboardingComplete()) {
          const target = library.includes(backendActive) ? backendActive : library[0];
          try {
            await cmd.setActiveGame(target);
            useAppStore.getState().setActiveGame(target);
            useAppStore.getState().navigateToGame(target);
          } catch {
            // Refused (session still active, e.g. webview reload): show the game
            // the backend is actually syncing, not one it refused to switch to.
            useAppStore.getState().navigateToGame(backendActive);
          }
        }
      } catch (e) {
        console.error("Failed to initialize:", e);
      }
      setReady(true);

      // Silent auto-update check
      try {
        const update = await check();
        if (update) {
          toast(`Update v${update.version} available`, {
            duration: Infinity,
            action: {
              label: "Update Now",
              onClick: async () => {
                toast.loading("Downloading update...", { id: "update" });
                try {
                  // Disconnect active session to release TCP connections
                  try { await cmd.disconnect(); } catch {}
                  await update.downloadAndInstall();
                  await relaunch();
                } catch {
                  toast.error("Update failed. Try again from Settings.", { id: "update" });
                }
              },
            },
          });
        }
      } catch {
        // Silent fail — user can still check manually in Settings
      }
    }

    if (isDemoMode()) {
      // Minimal game definitions for demo mode
      const demoRegistry = [
        {
          id: "sims4", label: "The Sims 4", family: "sims", icon: "gamepad-2",
          color: "text-accent-light", primary_color: "#1fb87e", steam_app_id: 1222670, auto_detect: true,
          content_types: [
            { id: "mods", label: "Script Mods", icon: "package", color: "text-accent-light", folder: "Mods", extensions: ["package", "ts4script", "zip"], file_type: "CustomContent", classify_by_extension: { ts4script: "Mod", zip: "Mod" }, syncable: true },
            { id: "saves", label: "Save Files", icon: "save", color: "text-status-green", folder: "Saves", extensions: [], file_type: "Save", syncable: true },
            { id: "tray", label: "Tray Items", icon: "layout-grid", color: "text-purple-400", folder: "Tray", extensions: [], file_type: "Tray", syncable: true },
            { id: "screenshots", label: "Screenshots", icon: "camera", color: "text-sky-400", folder: "Screenshots", extensions: [], file_type: "Screenshot", syncable: true },
          ],
          dangerous_script_extensions: ["ts4script"], packs: "sims4", legacy_id: "Sims4", disable_method: "rename", duplicate_finder: true,
        },
        {
          id: "minecraft_java", label: "Minecraft Java", family: "minecraft", icon: "box",
          color: "text-green-400", primary_color: "#4ade80", art_urls: {"cover": "https://store-images.s-microsoft.com/image/apps.808.14492077886571533.be42f4bd-887b-4430-8ed0-622341b4d2b0.c8274c53-105e-478b-9f4b-41b8088210a3", "hero": "https://store-images.s-microsoft.com/image/apps.58378.14492077886571533.338a563a-86e7-47b1-b9dc-41cf411f5dcd.dc840f22-6e8f-4a59-b7bc-57958a0740fd"}, auto_detect: true,
          content_types: [
            { id: "mods", label: "Mods", icon: "package", color: "text-green-400", folder: "mods", extensions: ["jar"], file_type: "Mod", syncable: true },
            { id: "saves", label: "Worlds", icon: "globe", color: "text-status-green", folder: "saves", extensions: [], file_type: "Save", syncable: true },
            { id: "resourcepacks", label: "Resource Packs", icon: "palette", color: "text-purple-400", folder: "resourcepacks", extensions: ["zip"], file_type: "ResourcePack", syncable: true },
            { id: "shaderpacks", label: "Shader Packs", icon: "sun", color: "text-sky-400", folder: "shaderpacks", extensions: ["zip"], file_type: "ShaderPack", syncable: true },
          ],
          dangerous_script_extensions: ["jar"],
        },
        {
          id: "wow_retail", label: "WoW Retail", family: "wow", icon: "swords",
          color: "text-yellow-400", primary_color: "#facc15", art_urls: {"hero": "https://blz-contentstack-images.akamaized.net/v3/assets/blt9c12f249ac15c7ec/bltb5a24e5ab1e2cfb0/6a88e3589b942efdb6f74110/wow-thumbnail-homepage.jpg"}, auto_detect: true,
          content_types: [
            { id: "addons", label: "Addons", icon: "package", color: "text-yellow-400", folder: "Interface/AddOns", extensions: ["lua", "toc", "xml"], file_type: "Addon", syncable: true },
            { id: "settings", label: "Settings", icon: "settings", color: "text-blue-400", folder: "WTF", extensions: ["lua", "bak"], file_type: "Settings", syncable: true },
          ],
          dangerous_script_extensions: [], legacy_id: "WowRetail",
        },
      ] as any[];
      setGameRegistry(demoRegistry);
      useAppStore.setState({
        gameRegistry: demoRegistry,
        manifest: demoManifests.sims4,
        session: demoSession,
        syncPlan: demoSyncPlan,
        profiles: demoProfiles,
        gamePaths: {
          sims4: "C:\\Users\\Player\\Documents\\Electronic Arts\\The Sims 4",
          minecraft_java: "C:\\Users\\Player\\AppData\\Roaming\\.minecraft",
          wow_retail: "C:\\Program Files\\World of Warcraft\\_retail_",
        },
        activeGame: "sims4",
        selectedGame: "sims4",
        myLibrary: ["sims4", "minecraft_java", "wow_retail"],
        installedGames: ["sims4", "minecraft_java", "wow_retail", "stardew_valley"],
        page: "dashboard",
      });
      useLogStore.setState({ logs: demoLogs });
      // The browser/welcome screens need the full catalog: load the real registry
      // (demo only, split into its own chunk) and append games not defined above.
      import("../src-tauri/src/game_registry.json").then(({ default: full }) => {
        const fullGames = (full as { games: any[] }).games;
        const byId = new Map(fullGames.map((g) => [g.id, g]));
        const merged = [
          ...demoRegistry.map((g) => ({ ...g, genres: byId.get(g.id)?.genres ?? [] })),
          ...fullGames.filter((g) => !demoRegistry.some((d) => d.id === g.id)),
        ];
        setGameRegistry(merged);
        useAppStore.setState((st) => ({
          gameRegistry: merged,
          gamePaths: { ...st.gamePaths, stardew_valley: "C:\\Games\\Stardew Valley", valheim: "C:\\Games\\Valheim" },
        }));
      });

      // Demo variants for screenshots / UI work: ?demo&offline, &welcome, &light, &page=content
      const q = new URLSearchParams(window.location.search);
      if (q.has("offline")) useAppStore.setState({ session: null, syncPlan: null });
      if (q.has("welcome")) {
        try { localStorage.removeItem("synccrate-onboarding-complete"); } catch {}
        useAppStore.setState({ selectedGame: null, myLibrary: [] });
      }
      if (q.has("light")) useAppStore.getState().setTheme("light");
      // &big: ~8,000 synthetic CC files to exercise the virtual content list.
      if (q.has("big")) {
        const big = bigSims4Demo();
        demoManifests.sims4 = big.manifest;
        useAppStore.setState({ manifest: big.manifest, modTags: big.tags });
      }
      // Appearance variants: &accent=%238b5cf6, &matchgame, &scale=1.25, &compact, &nofx
      const accent = q.get("accent");
      const scale = Number(q.get("scale"));
      useAppStore.getState().setAppearance({
        ...(accent && /^#[0-9a-f]{6}$/i.test(accent) ? { accent: accent.toLowerCase() } : {}),
        ...(q.has("matchgame") ? { matchGame: true } : {}),
        ...([0.9, 1.1, 1.25].includes(scale) ? { scale: scale as 0.9 | 1.1 | 1.25 } : {}),
        ...(q.has("compact") ? { density: "compact" as const } : {}),
        ...(q.has("nofx") ? { effects: false } : {}),
      });
      const demoPage = q.get("page");
      if (demoPage) useAppStore.setState({ page: demoPage as any });
      // &scroll=4000: start scrolled down (headless screenshots of long lists).
      const scrollTo = Number(q.get("scroll"));
      // Headless Edge only dispatches scroll events on real frames, so fire one by hand.
      if (scrollTo > 0) setTimeout(() => {
        const main = document.querySelector("main");
        main?.scrollTo(0, scrollTo);
        main?.dispatchEvent(new Event("scroll"));
      }, 1500);

      // In demo mode, swap manifests when the selected game changes
      useAppStore.subscribe((state, prev) => {
        if (state.selectedGame !== prev.selectedGame && state.selectedGame) {
          const m = demoManifests[state.selectedGame];
          if (m) state.setManifest(m);
          else state.setManifest({ files: {}, generated_at: Math.floor(Date.now() / 1000) });
        }
      });

    } else {
      init();
    }
  }, []);

  // Game the last drop went to: "Overwrite"/"Rename" in the results modal must
  // target it even if the user switched games meanwhile.
  const [dropGame, setDropGame] = useState<string | null>(null);

  const handleDrop = useCallback(
    async (e: Event) => {
      const paths = (e as CustomEvent<string[]>).detail;
      if (!paths || paths.length === 0) return;

      if (paths.length === 1 && paths[0].toLowerCase().endsWith(".scpack")) {
        if (!useAppStore.getState().selectedGame) {
          toastInfo("Open a game first, then drop a pack file");
          return;
        }
        useAppStore.getState().setPendingImportPackPath(paths[0]);
        useAppStore.getState().setPage("modpacks");
        return;
      }

      const { selectedGame: gameId, activeGame, session } = useAppStore.getState();
      if (!gameId) {
        // Without a game the backend fell back to the active game's folder.
        toastInfo("Open a game first, then drop its mod files");
        return;
      }
      if (gameId !== activeGame && session && session.session_type !== "None") {
        toastInfo(`You're in a session for ${gameLabel(activeGame)}. Disconnect to install ${gameLabel(gameId)} files.`);
        return;
      }
      try {
        const results = await cmd.installModFiles(paths, gameId);
        setDropGame(gameId);
        setInstallResults(results);
        const successCount = results.filter((r) => r.status === "Success").length;
        if (successCount > 0) {
          addLog(`Installed ${successCount} mod file(s)`, "success");
          toastSuccess(`Installed ${successCount} mod file(s)`);
          refreshIfShown(gameId);
        }
      } catch (e) {
        addLog(`Install failed: ${e}`, "error");
        toastError("Install failed");
      }
    },
    [addLog],
  );

  useEffect(() => {
    window.addEventListener("synccrate-drop", handleDrop);
    return () => window.removeEventListener("synccrate-drop", handleDrop);
  }, [handleDrop]);

  // Escape key closes install results modal
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && installResults) {
        setInstallResults(null);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [installResults]);

  const handleResolveDuplicate = async (source: string, strategy: "overwrite" | "rename") => {
    const gameId = dropGame;
    if (!gameId) return;
    try {
      const result = await cmd.confirmInstallDuplicate(source, strategy, gameId);
      setInstallResults((prev) =>
        prev
          ? prev.map((r) => (r.source === source ? result : r))
          : null,
      );
      if (result.status === "Success") {
        addLog(`Installed ${source.split(/[/\\]/).pop()} (${strategy})`, "success");
        refreshIfShown(gameId);
      }
    } catch (e) {
      addLog(`Install failed: ${e}`, "error");
    }
  };

  const renderPage = () => {
    // Wait for backend init before rendering content
    if (!ready && !isDemoMode()) return null;

    // Global pages (no game required)
    switch (page) {
      case "activity":
        return <ActivityLog />;
      case "settings":
        return <Settings />;
      case "game-browser":
        return <GameBrowser />;
    }

    // No game selected: show onboarding on first run, or game browser after
    if (!selectedGame) {
      if (!isOnboardingComplete()) return <WelcomeScreen />;
      return <GameBrowser />;
    }
    switch (page) {
      case "dashboard":
        return <GameDashboard gameId={selectedGame} />;
      case "content":
        return <ContentBrowser gameId={selectedGame} />;
      case "profiles":
        return <ProfileList gameId={selectedGame} />;
      case "backups":
        return <BackupList gameId={selectedGame} />;
      case "modpacks":
        return <ModpackList gameId={selectedGame} />;
      default:
        return <GameDashboard gameId={selectedGame} />;
    }
  };

  return (
    <Layout>
      {renderPage()}
      {isDragging && <DropZoneOverlay />}
      {installResults && (
        <InstallResultsModal
          results={installResults}
          onClose={() => setInstallResults(null)}
          onResolveDuplicate={handleResolveDuplicate}
        />
      )}
      <Toaster
        position="bottom-right"
        theme={theme}
        offset={20}
        toastOptions={{ classNames: { toast: "sc-toast" } }}
      />
    </Layout>
  );
}

export default App;
