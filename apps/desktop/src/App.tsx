import { useEffect, useState } from "react";
import { AppHeader } from "./components/AppHeader";
import { AppSidebar } from "./components/AppSidebar";
import { Dashboard } from "./components/Dashboard";
import { DeployPanel } from "./components/DeployPanel";
import { RestorePanel } from "./components/RestorePanel";
import { SetupPanel } from "./components/SetupPanel";
import { getFoundationStatus, previewStatus, type FoundationStatus } from "./shared/commands";
import { usePreferences } from "./shared/usePreferences";

export type ViewId = "overview" | "setup" | "restore" | "deploy";

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
        <AppHeader preferences={preferences} version={status.version} />
        {view === "overview" ? (
          <Dashboard status={status} t={preferences.t} onStartSetup={() => setView("setup")} />
        ) : view === "setup" ? (
          <SetupPanel t={preferences.t} />
        ) : view === "restore" ? (
          <RestorePanel t={preferences.t} />
        ) : (
          <DeployPanel t={preferences.t} />
        )}
      </div>
    </div>
  );
}
