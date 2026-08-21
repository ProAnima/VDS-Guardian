import { Settings2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Translate } from "../i18n";
import { CapturePlanPanel } from "./CapturePlanPanel";
import { RepositoryPanel } from "./RepositoryPanel";
import { RecoveryBundlePanel } from "./RecoveryBundlePanel";
import { RecoveryImportPanel } from "./RecoveryImportPanel";
import { SigningIdentityPanel } from "./SigningIdentityPanel";
import { SetupStatusPanel } from "./SetupStatusPanel";

interface SetupPanelProps {
  onManageServers: () => void;
  t: Translate;
}

export function SetupPanel({ onManageServers, t }: SetupPanelProps) {
  const [resourcesRevision, setResourcesRevision] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTarget, setSettingsTarget] = useState<"protection" | "storage">();
  const protectionRef = useRef<HTMLDivElement>(null);
  const storageRef = useRef<HTMLDivElement>(null);
  const resourcesChanged = () => setResourcesRevision((current) => current + 1);
  useEffect(() => {
    if (!settingsOpen || !settingsTarget) return;
    const frame = requestAnimationFrame(() => {
      (settingsTarget === "protection" ? protectionRef : storageRef).current?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
    return () => cancelAnimationFrame(frame);
  }, [settingsOpen, settingsTarget]);
  const openSettings = (target: "protection" | "storage") => { setSettingsTarget(target); setSettingsOpen(true); };
  return (
    <main className="dashboard">
      <section className="hero-panel backup-hero">
        <div className="hero-panel__content">
          <h1>{t("backupHeroTitle")}</h1>
          <p>{t("backupHeroBody")}</p>
        </div>
      </section>
      <SetupStatusPanel onManageServers={onManageServers} onOpenSettings={openSettings} resourcesRevision={resourcesRevision} t={t} />
      <details className="backup-settings" open={settingsOpen} onToggle={(event) => setSettingsOpen(event.currentTarget.open)}>
        <summary><Settings2 size={17} />{t("backupSettingsTitle")}</summary>
        <p>{t("backupSettingsBody")}</p>
        <div className="backup-settings__content">
          <div className="backup-settings__target" ref={protectionRef}><SigningIdentityPanel onIdentityChanged={resourcesChanged} t={t} /></div>
          <div className="backup-settings__target" ref={storageRef}><RepositoryPanel onRepositoriesChanged={resourcesChanged} t={t} /></div>
          <RecoveryBundlePanel resourcesRevision={resourcesRevision} t={t} />
          <RecoveryImportPanel onRepositoriesChanged={resourcesChanged} t={t} />
        </div>
      </details>
      <CapturePlanPanel onPlansChanged={resourcesChanged} resourcesRevision={resourcesRevision} t={t} />
    </main>
  );
}
