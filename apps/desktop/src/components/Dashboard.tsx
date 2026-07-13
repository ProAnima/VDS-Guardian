import {
  Archive, ArrowUpRight, Check, CircleDashed, HardDrive, LockKeyhole,
  Plus, RotateCcw, Server, ShieldCheck,
} from "lucide-react";
import type { Translate } from "../i18n";
import type { FoundationStatus } from "../shared/commands";
import { StatusCard } from "./StatusCard";

interface DashboardProps {
  status: FoundationStatus;
  t: Translate;
}

export function Dashboard({ status, t }: DashboardProps) {
  return (
    <main className="dashboard">
      <Hero status={status} t={t} />
      <StatusGrid t={t} />
      <div className="dashboard__columns">
        <ServersPanel t={t} />
        <RoadmapPanel t={t} />
      </div>
      <SecurityBanner t={t} />
      <footer className="app-footer"><span>{t("footerPlatform")}</span><span>{t("footerLicense")}</span></footer>
    </main>
  );
}

function Hero({ status, t }: DashboardProps) {
  return (
    <section className="hero-panel">
      <div className="hero-panel__content">
        <p className="eyebrow"><ShieldCheck size={15} aria-hidden="true" />{t("pageEyebrow")}</p>
        <h1>{t("pageTitle")}</h1>
        <p>{t("pageDescription")}</p>
        <div className="hero-panel__actions">
          <button className="button button--primary" type="button" disabled><Plus size={17} />{t("addServer")}</button>
          <button className="button button--secondary" type="button" disabled><Archive size={17} />{t("runBackup")}</button>
        </div>
      </div>
      <div className="safety-lock">
        <span className="safety-lock__icon"><LockKeyhole size={23} aria-hidden="true" /></span>
        <div><strong>{t("lockedTitle")}</strong><p>{t("lockedBody")}</p></div>
        <code>{status.iteration}</code>
      </div>
    </section>
  );
}

function StatusGrid({ t }: { t: Translate }) {
  return (
    <section className="status-grid" aria-label={t("pageEyebrow")}>
      <StatusCard icon={ShieldCheck} label={t("statusProtection")} value={t("statusFoundation")} tone="amber" />
      <StatusCard icon={HardDrive} label={t("statusNodes")} value={t("statusOneNode")} tone="green" />
      <StatusCard icon={Check} label={t("statusVerified")} value={t("statusUnavailable")} tone="neutral" />
      <StatusCard icon={RotateCcw} label={t("statusRecovery")} value={t("statusConfiguration")} tone="neutral" />
    </section>
  );
}

function ServersPanel({ t }: { t: Translate }) {
  return (
    <section className="content-panel servers-panel">
      <PanelHeader title={t("serversTitle")} action={t("serversAction")} />
      <div className="empty-state">
        <div className="empty-state__visual"><Server size={31} strokeWidth={1.6} /><span /><span /></div>
        <h2>{t("emptyTitle")}</h2>
        <p>{t("emptyBody")}</p>
        <button type="button" className="text-button" disabled><span>{t("emptyAction")}</span><ArrowUpRight size={15} /></button>
      </div>
    </section>
  );
}

function RoadmapPanel({ t }: { t: Translate }) {
  const items = [
    [t("roadmapFoundation"), t("statusReady"), "ready"],
    [t("roadmapDomain"), t("statusInProgress"), "current"],
    [t("roadmapRemote"), t("statusPlanned"), "planned"],
    [t("roadmapRestore"), t("statusPlanned"), "planned"],
  ] as const;
  return (
    <section className="content-panel roadmap-panel">
      <PanelHeader title={t("roadmapTitle")} />
      <ol className="roadmap-list">
        {items.map(([label, state, tone], index) => (
          <li key={label} data-tone={tone}>
            <span className="roadmap-list__number">0{index + 1}</span>
            <div><strong>{label}</strong><span>{state}</span></div>
            {tone === "ready" ? <Check size={16} /> : <CircleDashed size={16} />}
          </li>
        ))}
      </ol>
    </section>
  );
}

function PanelHeader({ title, action }: { title: string; action?: string }) {
  return <header className="panel-header"><h2>{title}</h2>{action && <button type="button" disabled>{action}</button>}</header>;
}

function SecurityBanner({ t }: { t: Translate }) {
  return (
    <section className="security-banner">
      <span><LockKeyhole size={20} aria-hidden="true" /></span>
      <div><strong>{t("securityTitle")}</strong><p>{t("securityBody")}</p></div>
    </section>
  );
}
