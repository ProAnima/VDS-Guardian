import { useEffect, useState, type ReactNode } from "react";
import { AppHeader } from "./components/AppHeader";
import { AppSidebar } from "./components/AppSidebar";
import { Dashboard } from "./components/Dashboard";
import { RestorePanel } from "./components/RestorePanel";
import { SetupPanel } from "./components/SetupPanel";
import { ServersPanel } from "./components/ServersPanel";
import { getFoundationStatus, previewStatus, type FoundationStatus } from "./shared/commands";
import { usePreferences } from "./shared/usePreferences";

export type ViewId = "overview" | "servers" | "backup" | "restore";

const viewIds: ViewId[] = ["overview", "servers", "backup", "restore"];

/**
 * Views are mounted on their first visit and then kept (hidden) so a running backup or restore,
 * its Cancel button and the explorer state survive switching views. Every visit bumps the view's
 * refresh key so its data is re-read quietly when the operator comes back.
 */
function useViews() {
  const [view, setView] = useState<ViewId>("overview");
  const [visits, setVisits] = useState<Record<ViewId, number>>({ overview: 1, servers: 0, backup: 0, restore: 0 });
  const show = (next: ViewId) => { setView(next); setVisits((current) => ({ ...current, [next]: current[next] + 1 })); };
  return { view, visits, show };
}

export function App() {
  const preferences = usePreferences();
  const { t } = preferences;
  const [status, setStatus] = useState<FoundationStatus>(previewStatus);
  const { view, visits, show } = useViews();
  const [setupTarget, setSetupTarget] = useState<"protection" | "storage">();
  const navigate = (next: ViewId) => { setSetupTarget(undefined); show(next); };

  useEffect(() => {
    void getFoundationStatus().then(setStatus);
  }, []);

  const content: Record<ViewId, () => ReactNode> = {
    overview: () => (
      <Dashboard
        status={status} t={t} refreshKey={visits.overview}
        onAddServer={() => navigate("servers")}
        onRunBackup={() => navigate("backup")}
        onRestore={() => navigate("restore")}
        onOpenSetup={(target) => { setSetupTarget(target); show("backup"); }}
      />
    ),
    servers: () => <ServersPanel refreshKey={visits.servers} t={t} />,
    backup: () => <SetupPanel onManageServers={() => navigate("servers")} initialTarget={setupTarget} refreshKey={visits.backup} t={t} />,
    restore: () => <RestorePanel onManageBackups={() => navigate("backup")} onManageServers={() => navigate("servers")} refreshKey={visits.restore} t={t} />,
  };

  return (
    <div className="app-frame">
      <AppSidebar t={t} activeView={view} onNavigate={navigate} />
      <div className="app-workspace">
        <AppHeader preferences={preferences} />
        {viewIds.filter((id) => visits[id] > 0).map((id) => (
          <div className="view-host" hidden={view !== id} key={id}>{content[id]()}</div>
        ))}
      </div>
    </div>
  );
}
