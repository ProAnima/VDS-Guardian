import { useEffect, useState } from "react";
import { AppHeader } from "./components/AppHeader";
import { AppSidebar } from "./components/AppSidebar";
import { Dashboard } from "./components/Dashboard";
import { RestorePanel } from "./components/RestorePanel";
import { getFoundationStatus, previewStatus, type FoundationStatus } from "./shared/commands";
import { usePreferences } from "./shared/usePreferences";

export type ViewId = "overview" | "restore";

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
          <Dashboard status={status} t={preferences.t} />
        ) : (
          <RestorePanel t={preferences.t} />
        )}
      </div>
    </div>
  );
}
