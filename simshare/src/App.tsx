import { useEffect, useState, useCallback } from "react";
import { Toaster } from "sonner";
import Layout from "./components/Layout";
import Dashboard from "./components/Dashboard";
import ModList from "./components/ModList";
import SaveList from "./components/SaveList";
import ProfileList from "./components/ProfileList";
import BackupList from "./components/BackupList";
import ActivityLog from "./components/ActivityLog";
import Settings from "./components/Settings";
import DropZoneOverlay from "./components/DropZoneOverlay";
import InstallResultsModal from "./components/InstallResultsModal";
import { useAppStore } from "./stores/useAppStore";
import { useLogStore } from "./stores/useLogStore";
import { useTauriEvents } from "./hooks/useTauriEvents";
import {
  isDemoMode,
  demoManifest,
  demoSession,
  demoSyncPlan,
  demoProfiles,
  demoLogs,
} from "./lib/demoData";
import type { InstallResult } from "./lib/types";
import { toastSuccess, toastError } from "./lib/toast";
import * as cmd from "./lib/commands";

function App() {
  const page = useAppStore((s) => s.page);
  const isDragging = useAppStore((s) => s.isDragging);
  const addLog = useLogStore((s) => s.addLog);
  useTauriEvents();

  const [installResults, setInstallResults] = useState<InstallResult[] | null>(null);

  useEffect(() => {
    if (isDemoMode()) {
      useAppStore.setState({
        manifest: demoManifest,
        session: demoSession,
        syncPlan: demoSyncPlan,
        profiles: demoProfiles,
        gamePaths: { Sims4: "C:\\Users\\Player\\Documents\\Electronic Arts\\The Sims 4" },
        activeGame: "Sims4" as const,
      });
      useLogStore.setState({ logs: demoLogs });
    }
  }, []);

  const handleDrop = useCallback(
    async (e: Event) => {
      const paths = (e as CustomEvent<string[]>).detail;
      if (!paths || paths.length === 0) return;
      try {
        const results = await cmd.installModFiles(paths);
        setInstallResults(results);
        const successCount = results.filter((r) => r.status === "Success").length;
        if (successCount > 0) {
          addLog(`Installed ${successCount} mod file(s)`, "success");
          toastSuccess(`Installed ${successCount} mod file(s)`);
          cmd.scanFiles().then((m) => useAppStore.getState().setManifest(m)).catch(() => {});
        }
      } catch (e) {
        addLog(`Install failed: ${e}`, "error");
        toastError("Install failed");
      }
    },
    [addLog],
  );

  useEffect(() => {
    window.addEventListener("simshare-drop", handleDrop);
    return () => window.removeEventListener("simshare-drop", handleDrop);
  }, [handleDrop]);

  const handleResolveDuplicate = async (source: string, strategy: "overwrite" | "rename") => {
    try {
      const result = await cmd.confirmInstallDuplicate(source, strategy);
      setInstallResults((prev) =>
        prev
          ? prev.map((r) => (r.source === source ? result : r))
          : null,
      );
      if (result.status === "Success") {
        addLog(`Installed ${source.split(/[/\\]/).pop()} (${strategy})`, "success");
        cmd.scanFiles().then((m) => useAppStore.getState().setManifest(m)).catch(() => {});
      }
    } catch (e) {
      addLog(`Install failed: ${e}`, "error");
    }
  };

  const renderPage = () => {
    switch (page) {
      case "dashboard":
        return <Dashboard />;
      case "mods":
        return <ModList />;
      case "saves":
        return <SaveList />;
      case "profiles":
        return <ProfileList />;
      case "backups":
        return <BackupList />;
      case "activity":
        return <ActivityLog />;
      case "settings":
        return <Settings />;
      default:
        return <Dashboard />;
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
