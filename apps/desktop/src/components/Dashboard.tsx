import { Archive, ArrowUpRight, CircleAlert, LoaderCircle, LockKeyhole, RotateCcw, Server, ShieldCheck, type LucideIcon } from "lucide-react";
import type { Translate } from "../i18n";
import type { FoundationStatus } from "../shared/commands";
import { evaluateOverviewReadiness } from "./setup-readiness";
import { useSetupStatus } from "./use-setup-status";

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
  const setup = useSetupStatus(0, t);
  const locked = !status.liveOperationsEnabled;
  const verdict = evaluateOverviewReadiness({ resources: setup.resources, failureCount: setup.failures.length, loading: setup.loading }, t);
  const state = locked ? "locked" : verdict.state;
  const Icon = { locked: LockKeyhole, ready: ShieldCheck, attention: CircleAlert, loading: LoaderCircle }[state];
  return (
    <aside className="overview-status" data-ready={state === "ready" || undefined} data-state={state} aria-live="polite">
      <span><Icon className={state === "loading" ? "spin" : undefined} size={20} aria-hidden="true" /></span>
      <div><strong>{locked ? t("lockedTitle") : verdict.title}</strong><p>{locked ? t("lockedBody") : verdict.body}</p></div>
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
