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
  const [setupTarget, setSetupTarget] = useState<"protection" | "storage">();
  const navigate = (next: ViewId) => { setSetupTarget(undefined); setView(next); };

  useEffect(() => {
    void getFoundationStatus().then(setStatus);
  }, []);

  return (
    <div className="app-frame">
      <AppSidebar t={preferences.t} activeView={view} onNavigate={navigate} />
      <div className="app-workspace">
        <AppHeader preferences={preferences} />
        {view === "overview" ? (
          <Dashboard
            status={status}
            t={preferences.t}
            onAddServer={() => navigate("servers")}
            onRunBackup={() => navigate("backup")}
            onRestore={() => navigate("restore")}
            onOpenSetup={(target) => { setSetupTarget(target); setView("backup"); }}
          />
        ) : view === "servers" ? (
          <ServersPanel t={preferences.t} />
        ) : view === "backup" ? (
          <SetupPanel onManageServers={() => navigate("servers")} initialTarget={setupTarget} t={preferences.t} />
        ) : (
          <RestorePanel onManageBackups={() => navigate("backup")} onManageServers={() => navigate("servers")} t={preferences.t} />
        )}
      </div>
    </div>
  );
}
