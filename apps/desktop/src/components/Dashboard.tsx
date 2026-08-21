import { Archive, ArrowUpRight, LockKeyhole, RotateCcw, Server, ShieldCheck, type LucideIcon } from "lucide-react";
import type { Translate } from "../i18n";
import type { FoundationStatus } from "../shared/commands";

interface DashboardProps {
  status: FoundationStatus;
  t: Translate;
  onAddServer: () => void;
  onRunBackup: () => void;
  onRestore: () => void;
}

export function Dashboard(props: DashboardProps) {
  const { status, t, onAddServer, onRunBackup, onRestore } = props;
  return (
    <main className="dashboard dashboard--overview">
      <section className="overview-hero">
        <div className="overview-hero__content">
          <p className="eyebrow"><ShieldCheck size={15} aria-hidden="true" />{t("pageEyebrow")}</p>
          <h1>{t("pageTitle")}</h1>
          <p>{t("pageDescription")}</p>
          <button className="button button--primary" type="button" onClick={onRunBackup}>
            <Archive size={17} aria-hidden="true" />{t("runBackup")}
          </button>
        </div>
        <SafetyStatus status={status} t={t} />
      </section>
      <section className="workflow-panel" aria-labelledby="workflow-title">
        <header><span>01—03</span><h2 id="workflow-title">{t("dashboardStartTitle")}</h2></header>
        <div className="workflow-grid">
          <WorkflowStep index="01" icon={Server} title={t("navServers")} body={t("serversBody")} onClick={onAddServer} />
          <WorkflowStep index="02" icon={Archive} title={t("navBackups")} body={t("backupHeroBody")} onClick={onRunBackup} />
          <WorkflowStep index="03" icon={RotateCcw} title={t("navRestore")} body={t("restoreBody")} onClick={onRestore} />
        </div>
      </section>
    </main>
  );
}

function SafetyStatus({ status, t }: Pick<DashboardProps, "status" | "t">) {
  const ready = status.liveOperationsEnabled;
  const Icon = ready ? ShieldCheck : LockKeyhole;
  return (
    <aside className="overview-status" data-ready={ready || undefined}>
      <span><Icon size={20} aria-hidden="true" /></span>
      <div><strong>{t(ready ? "statusReady" : "lockedTitle")}</strong><p>{t(ready ? "securityBody" : "lockedBody")}</p></div>
    </aside>
  );
}

interface WorkflowStepProps {
  index: string;
  icon: LucideIcon;
  title: string;
  body: string;
  onClick: () => void;
}

function WorkflowStep({ index, icon: Icon, title, body, onClick }: WorkflowStepProps) {
  return (
    <button className="workflow-step" type="button" onClick={onClick}>
      <span className="workflow-step__index">{index}</span>
      <span className="workflow-step__icon"><Icon size={19} aria-hidden="true" /></span>
      <strong>{title}</strong><small>{body}</small>
      <ArrowUpRight className="workflow-step__arrow" size={16} aria-hidden="true" />
    </button>
  );
}
