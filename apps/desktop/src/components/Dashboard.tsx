import { Archive, ArrowUpRight, LockKeyhole, Plus, Server, ShieldCheck } from "lucide-react";
import type { Translate } from "../i18n";
import type { FoundationStatus } from "../shared/commands";
interface DashboardProps {
  status: FoundationStatus;
  t: Translate;
  onAddServer: () => void;
  onRunBackup: () => void;
}

export function Dashboard({ status, t, onAddServer, onRunBackup }: DashboardProps) {
  return (
    <main className="dashboard">
      <Hero status={status} t={t} onAddServer={onAddServer} onRunBackup={onRunBackup} />
      <SetupPanel t={t} onAddServer={onAddServer} />
      <SecurityBanner t={t} />
      <footer className="app-footer"><span>{t("footerPlatform")}</span><span>{t("footerLicense")}</span></footer>
    </main>
  );
}

function Hero({ status, t, onAddServer, onRunBackup }: DashboardProps) {
  return (
    <section className="hero-panel">
      <div className="hero-panel__content">
        <p className="eyebrow"><ShieldCheck size={15} aria-hidden="true" />{t("pageEyebrow")}</p>
        <h1>{t("pageTitle")}</h1>
        <p>{t("pageDescription")}</p>
        <div className="hero-panel__actions">
          <button className="button button--primary" type="button" onClick={onAddServer}><Plus size={17} />{t("addServer")}</button>
          <button className="button button--secondary" type="button" onClick={onRunBackup}><Archive size={17} />{t("runBackup")}</button>
        </div>
      </div>
      <div className="safety-lock">
        <span className="safety-lock__icon">
          {status.liveOperationsEnabled
            ? <ShieldCheck size={23} aria-hidden="true" />
            : <LockKeyhole size={23} aria-hidden="true" />}
        </span>
        <div>
          <strong>{t(status.liveOperationsEnabled ? "statusReady" : "lockedTitle")}</strong>
          <p>{t(status.liveOperationsEnabled ? "securityBody" : "lockedBody")}</p>
        </div>
        <code>{status.iteration}</code>
      </div>
    </section>
  );
}

function SetupPanel({ t, onAddServer }: Pick<DashboardProps, "t" | "onAddServer">) {
  return (
    <section className="content-panel servers-panel">
      <PanelHeader title={t("setupHeroTitle")} action={t("serversAction")} onAction={onAddServer} />
      <div className="empty-state">
        <div className="empty-state__visual"><Server size={31} strokeWidth={1.6} /><span /><span /></div>
        <h2>{t("setupStepServer")}</h2>
        <p>{t("setupHeroBody")}</p>
        <button type="button" className="text-button" onClick={onAddServer}><span>{t("addServer")}</span><ArrowUpRight size={15} /></button>
      </div>
    </section>
  );
}

function PanelHeader({ title, action, onAction }: { title: string; action?: string; onAction?: () => void }) {
  return <header className="panel-header"><h2>{title}</h2>{action && <button type="button" disabled={!onAction} onClick={onAction}>{action}</button>}</header>;
}

function SecurityBanner({ t }: { t: Translate }) {
  return (
    <section className="security-banner">
      <span><LockKeyhole size={20} aria-hidden="true" /></span>
      <div><strong>{t("securityTitle")}</strong><p>{t("securityBody")}</p></div>
    </section>
  );
}
