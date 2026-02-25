import { useEffect } from "react";
import Layout from "./components/Layout";
import Dashboard from "./components/Dashboard";
import ModList from "./components/ModList";
import SaveList from "./components/SaveList";
import ProfileList from "./components/ProfileList";
import ActivityLog from "./components/ActivityLog";
import Settings from "./components/Settings";
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

function App() {
  const page = useAppStore((s) => s.page);
  useTauriEvents();

  useEffect(() => {
    if (isDemoMode()) {
      useAppStore.setState({
        manifest: demoManifest,
        session: demoSession,
        syncPlan: demoSyncPlan,
        profiles: demoProfiles,
        sims4Path: "C:\\Users\\Player\\Documents\\Electronic Arts\\The Sims 4",
      });
      useLogStore.setState({ logs: demoLogs });
    }
  }, []);

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
      case "activity":
        return <ActivityLog />;
      case "settings":
        return <Settings />;
      default:
        return <Dashboard />;
    }
  };

  return <Layout>{renderPage()}</Layout>;
}

export default App;
