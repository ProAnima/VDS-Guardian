import { CheckCircle2, FolderArchive, KeyRound, ListChecks, Server, type LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import type { Translate } from "../i18n";
import { CapturePlanPanel } from "./CapturePlanPanel";
import { RepositoryPanel } from "./RepositoryPanel";
import { RecoveryBundlePanel } from "./RecoveryBundlePanel";
import { RecoveryImportPanel } from "./RecoveryImportPanel";
import { SigningIdentityPanel } from "./SigningIdentityPanel";
import { SshProfilePanel } from "./SshProfilePanel";
import { SetupStatusPanel } from "./SetupStatusPanel";

interface SetupPanelProps {
  t: Translate;
}

export function SetupPanel({ t }: SetupPanelProps) {
  const [resourcesRevision, setResourcesRevision] = useState(0);
  const resourcesChanged = () => setResourcesRevision((current) => current + 1);
  return (
    <main className="dashboard">
      <section className="hero-panel">
        <div className="hero-panel__content">
          <p className="eyebrow"><ListChecks size={15} aria-hidden="true" />Настройка</p>
          <h1>Подготовить первый бэкап</h1>
          <p>Пройдите четыре шага по порядку. Ключи и recovery-материал не попадут в конфигурацию приложения.</p>
        </div>
      </section>
      <SetupStatusPanel resourcesRevision={resourcesRevision} />
      <SetupStep number="1" title="Идентичность подписи" icon={KeyRound}>
        <SigningIdentityPanel t={t} />
      </SetupStep>
      <SetupStep number="2" title="Хранилище и recovery" icon={FolderArchive}>
        <RepositoryPanel onRepositoriesChanged={resourcesChanged} />
        <RecoveryBundlePanel resourcesRevision={resourcesRevision} />
        <RecoveryImportPanel onRepositoriesChanged={resourcesChanged} />
      </SetupStep>
      <SetupStep number="3" title="Сервер для бэкапа" icon={Server}>
        <SshProfilePanel onProfilesChanged={resourcesChanged} />
      </SetupStep>
      <SetupStep number="4" title="Что сохраняем" icon={CheckCircle2}>
        <CapturePlanPanel resourcesRevision={resourcesRevision} t={t} />
      </SetupStep>
    </main>
  );
}

interface SetupStepProps {
  number: string;
  title: string;
  icon: LucideIcon;
  children: ReactNode;
}

function SetupStep({ number, title, icon: Icon, children }: SetupStepProps) {
  return (
    <section className="content-panel">
      <header className="panel-header"><h2><span className="roadmap-list__number">{number}</span> <Icon size={17} aria-hidden="true" /> {title}</h2></header>
      {children}
    </section>
  );
}
