import { Settings2 } from "lucide-react";
import { useState } from "react";
import type { Translate } from "../i18n";
import { CapturePlanPanel } from "./CapturePlanPanel";
import { RepositoryPanel } from "./RepositoryPanel";
import { RecoveryBundlePanel } from "./RecoveryBundlePanel";
import { RecoveryImportPanel } from "./RecoveryImportPanel";
import { SigningIdentityPanel } from "./SigningIdentityPanel";
import { SetupStatusPanel } from "./SetupStatusPanel";

interface SetupPanelProps {
  t: Translate;
}

export function SetupPanel({ t }: SetupPanelProps) {
  const [resourcesRevision, setResourcesRevision] = useState(0);
  const resourcesChanged = () => setResourcesRevision((current) => current + 1);
  return (
    <main className="dashboard">
      <section className="hero-panel backup-hero">
        <div className="hero-panel__content">
          <h1>{t("backupHeroTitle")}</h1>
          <p>{t("backupHeroBody")}</p>
        </div>
      </section>
      <SetupStatusPanel resourcesRevision={resourcesRevision} t={t} />
      <CapturePlanPanel onPlansChanged={resourcesChanged} resourcesRevision={resourcesRevision} t={t} />
      <details className="backup-settings">
        <summary><Settings2 size={17} />{t("backupSettingsTitle")}</summary>
        <p>{t("backupSettingsBody")}</p>
        <div className="backup-settings__content">
          <SigningIdentityPanel onIdentityChanged={resourcesChanged} t={t} />
          <RepositoryPanel onRepositoriesChanged={resourcesChanged} t={t} />
          <RecoveryBundlePanel resourcesRevision={resourcesRevision} t={t} />
          <RecoveryImportPanel onRepositoriesChanged={resourcesChanged} t={t} />
        </div>
      </details>
    </main>
  );
}
