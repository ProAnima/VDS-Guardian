import { useEffect, useState } from "react";
import { AppHeader } from "./components/AppHeader";
import { AppSidebar } from "./components/AppSidebar";
import { Dashboard } from "./components/Dashboard";
import { RestorePanel } from "./components/RestorePanel";
import { SetupPanel } from "./components/SetupPanel";
import { ServersPanel } from "./components/ServersPanel";
import { getFoundationStatus, previewStatus, type FoundationStatus } from "./shared/commands";
import { usePreferences } from "./shared/usePreferences";

export type ViewId = "overview" | "servers" | "backup" | "restore";

export function App() {
  const preferences = usePreferences();
  const [status, setStatus] = useState<FoundationStatus>(previewStatus);
  const [view, setView] = useState<ViewId>("overview");

  useEffect(() => {
    void getFoundationStatus().then(setStatus);
  }, []);

  return (
    <div className="app-frame">
      <AppSidebar t={preferences.t} activeView={view} onNavigate={setView} />
      <div className="app-workspace">
        <AppHeader preferences={preferences} />
        {view === "overview" ? (
          <Dashboard
            status={status}
            t={preferences.t}
            onAddServer={() => setView("servers")}
            onRunBackup={() => setView("backup")}
            onRestore={() => setView("restore")}
          />
        ) : view === "servers" ? (
          <ServersPanel t={preferences.t} />
        ) : view === "backup" ? (
          <SetupPanel onManageServers={() => setView("servers")} t={preferences.t} />
        ) : (
          <RestorePanel onManageBackups={() => setView("backup")} onManageServers={() => setView("servers")} t={preferences.t} />
        )}
      </div>
    </div>
  );
}
