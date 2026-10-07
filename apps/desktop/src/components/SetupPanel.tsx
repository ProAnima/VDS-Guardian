import { useEffect, useRef, useState } from "react";
import { Settings2, X } from "lucide-react";
import type { Translate } from "../i18n";
import { tip } from "../shared/tip";
import { BackupWorkspace } from "./backup/BackupWorkspace";
import { RecoveryBundlePanel } from "./RecoveryBundlePanel";
import { RecoveryImportPanel } from "./RecoveryImportPanel";
import { RepositoryPanel } from "./RepositoryPanel";
import { SetupStatusPanel } from "./SetupStatusPanel";
import { SigningIdentityPanel } from "./SigningIdentityPanel";

interface SetupPanelProps {
  onManageServers: () => void;
  t: Translate;
}

type SettingsTarget = "protection" | "storage";

export function SetupPanel({ onManageServers, t }: SetupPanelProps) {
  const [resourcesRevision, setResourcesRevision] = useState(0);
  const [settings, setSettings] = useState<{ open: boolean; target?: SettingsTarget }>({ open: false });
  const resourcesChanged = () => setResourcesRevision((current) => current + 1);
  const openSettings = (target?: SettingsTarget) => setSettings({ open: true, target });
  const closeSettings = () => setSettings({ open: false });
  return (
    <main className="view view--backup">
      <BackupWorkspace
        onPlansChanged={resourcesChanged} resourcesRevision={resourcesRevision} t={t}
        toolbarEnd={<button className="icon-button icon-button--large" type="button" data-active={settings.open || undefined} onClick={() => (settings.open ? closeSettings() : openSettings())} {...tip(t("backupSettingsTitle"))}><Settings2 size={17} aria-hidden="true" /></button>}
        notReady={<SetupStatusPanel onManageServers={onManageServers} onOpenSettings={openSettings} resourcesRevision={resourcesRevision} t={t} />}
      />
      {settings.open && <SettingsDrawer target={settings.target} onClose={closeSettings} onChanged={resourcesChanged} revision={resourcesRevision} t={t} />}
    </main>
  );
}

interface SettingsDrawerProps {
  target?: SettingsTarget;
  revision: number;
  onChanged: () => void;
  onClose: () => void;
  t: Translate;
}

function SettingsDrawer({ target, revision, onChanged, onClose, t }: SettingsDrawerProps) {
  const protectionRef = useRef<HTMLDivElement>(null);
  const storageRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const frame = requestAnimationFrame(() => (target === "storage" ? storageRef : protectionRef).current?.scrollIntoView({ block: "start" }));
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => { cancelAnimationFrame(frame); window.removeEventListener("keydown", onKey); };
  }, [onClose, target]);
  return (
    <aside className="drawer" role="dialog" aria-label={t("backupSettingsTitle")}>
      <header className="drawer__header">
        <Settings2 size={16} aria-hidden="true" /><strong>{t("backupSettingsTitle")}</strong>
        <button className="icon-button" type="button" onClick={onClose} {...tip(t("dismiss"))}><X size={15} aria-hidden="true" /></button>
      </header>
      <div className="drawer__content">
        <div ref={protectionRef}><SigningIdentityPanel onIdentityChanged={onChanged} t={t} /></div>
        <div ref={storageRef}><RepositoryPanel onRepositoriesChanged={onChanged} t={t} /></div>
        <RecoveryBundlePanel resourcesRevision={revision} t={t} />
        <RecoveryImportPanel onRepositoriesChanged={onChanged} t={t} />
      </div>
    </aside>
  );
}
