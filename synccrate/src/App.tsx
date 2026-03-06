import { useEffect, useState, useCallback } from "react";
import { Toaster } from "sonner";
import Layout from "./components/Layout";
import GameDashboard from "./components/GameDashboard";
import ContentBrowser from "./components/ContentBrowser";
import ProfileList from "./components/ProfileList";
import BackupList from "./components/BackupList";
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
} from "./lib/demoData";
import type { InstallResult } from "./lib/types";
import { toastSuccess, toastError } from "./lib/toast";
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

function App() {
  const page = useAppStore((s) => s.page);
  const selectedGame = useAppStore((s) => s.selectedGame);
  const isDragging = useAppStore((s) => s.isDragging);
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

        // Auto-select first library game if none selected (skip during onboarding)
        if (!useAppStore.getState().selectedGame && library.length > 0 && isOnboardingComplete()) {
          await cmd.setActiveGame(library[0]);
          useAppStore.getState().navigateToGame(library[0]);
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
          color: "text-accent-light", primary_color: "#1ea84b", auto_detect: true,
          content_types: [
            { id: "mods", label: "Script Mods", icon: "package", color: "text-accent-light", folder: "Mods", extensions: ["package", "ts4script", "zip"], file_type: "CustomContent", classify_by_extension: { ts4script: "Mod", zip: "Mod" }, syncable: true },
            { id: "saves", label: "Save Files", icon: "save", color: "text-status-green", folder: "Saves", extensions: [], file_type: "Save", syncable: true },
            { id: "tray", label: "Tray Items", icon: "layout-grid", color: "text-purple-400", folder: "Tray", extensions: [], file_type: "Tray", syncable: true },
            { id: "screenshots", label: "Screenshots", icon: "camera", color: "text-sky-400", folder: "Screenshots", extensions: [], file_type: "Screenshot", syncable: true },
          ],
          dangerous_script_extensions: ["ts4script"], packs: "sims4", legacy_id: "Sims4",
        },
        {
          id: "minecraft_java", label: "Minecraft Java", family: "minecraft", icon: "box",
          color: "text-green-400", primary_color: "#4ade80", auto_detect: true,
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
          color: "text-yellow-400", primary_color: "#facc15", auto_detect: true,
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
        page: "dashboard",
      });
      useLogStore.setState({ logs: demoLogs });

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

  const handleDrop = useCallback(
    async (e: Event) => {
      const paths = (e as CustomEvent<string[]>).detail;
      if (!paths || paths.length === 0) return;
      const gameId = useAppStore.getState().selectedGame;
      try {
        const results = await cmd.installModFiles(paths, gameId ?? undefined);
        setInstallResults(results);
        const successCount = results.filter((r) => r.status === "Success").length;
        if (successCount > 0) {
          addLog(`Installed ${successCount} mod file(s)`, "success");
          toastSuccess(`Installed ${successCount} mod file(s)`);
          cmd.scanFiles(gameId ?? undefined).then((m) => useAppStore.getState().setManifest(m)).catch(() => {});
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
    const gameId = useAppStore.getState().selectedGame;
    try {
      const result = await cmd.confirmInstallDuplicate(source, strategy, gameId ?? undefined);
      setInstallResults((prev) =>
        prev
          ? prev.map((r) => (r.source === source ? result : r))
          : null,
      );
      if (result.status === "Success") {
        addLog(`Installed ${source.split(/[/\\]/).pop()} (${strategy})`, "success");
        cmd.scanFiles(gameId ?? undefined).then((m) => useAppStore.getState().setManifest(m)).catch(() => {});
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
        theme="dark"
        toastOptions={{
          style: {
            background: "#121a22",
            border: "1px solid #1e2d38",
            color: "#e8ecf4",
          },
        }}
      />
    </Layout>
  );
}

export default App;
