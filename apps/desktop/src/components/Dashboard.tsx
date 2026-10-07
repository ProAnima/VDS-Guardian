import { Archive, CircleAlert, CircleCheck, KeyRound, LoaderCircle, LockKeyhole, RotateCcw, Server, ShieldCheck, type LucideIcon } from "lucide-react";
import type { Translate } from "../i18n";
import type { FoundationStatus } from "../shared/commands";
import { evaluateOverviewReadiness, evaluateSetupReadiness, type SetupStatusItem } from "./setup-readiness";
import { useSetupStatus } from "./use-setup-status";

interface DashboardProps {
  status: FoundationStatus;
  t: Translate;
  onAddServer: () => void;
  onRunBackup: () => void;
  onRestore: () => void;
  onOpenSetup?: (target: "protection" | "storage") => void;
  refreshKey?: number;
}

const targetIcon: Record<SetupStatusItem["target"], LucideIcon> = { protection: KeyRound, storage: Archive, servers: Server };

export function Dashboard(props: DashboardProps) {
  const { status, t } = props;
  const setup = useSetupStatus(props.refreshKey ?? 0, t);
  const items = setup.resources ? evaluateSetupReadiness(setup.resources, t) : [];
  const verdict = evaluateOverviewReadiness({ resources: setup.resources, failureCount: setup.failures.length, loading: setup.loading }, t);
  return (
    <main className="view overview">
      <SafetyStatus locked={!status.liveOperationsEnabled} verdict={verdict} t={t} />
      <section className="overview__readiness" aria-label={verdict.title}>
        {items.map((item) => <ReadinessTile key={item.target} item={item} props={props} />)}
      </section>
      <section className="overview__actions" aria-label={t("dashboardStartTitle")}>
        <ActionTile icon={Server} label={t("navServers")} onClick={props.onAddServer} />
        <ActionTile icon={Archive} label={t("navBackups")} onClick={props.onRunBackup} primary />
        <ActionTile icon={RotateCcw} label={t("navRestore")} onClick={props.onRestore} />
      </section>
    </main>
  );
}

function SafetyStatus({ locked, verdict, t }: { locked: boolean; verdict: ReturnType<typeof evaluateOverviewReadiness>; t: Translate }) {
  const state = locked ? "locked" : verdict.state;
  const Icon = { locked: LockKeyhole, ready: ShieldCheck, attention: CircleAlert, loading: LoaderCircle }[state];
  return (
    <aside className="overview-status" data-ready={state === "ready" || undefined} data-state={state} aria-live="polite" data-tip={locked ? t("lockedBody") : verdict.body} data-tip-side="start">
      <Icon className={state === "loading" ? "spin" : undefined} size={20} aria-hidden="true" />
      <strong>{locked ? t("lockedTitle") : verdict.title}</strong>
    </aside>
  );
}

function ReadinessTile({ item, props }: { item: SetupStatusItem; props: DashboardProps }) {
  const Icon = targetIcon[item.target];
  const ready = item.readiness === "ready";
  const open = () => {
    if (item.target === "servers") props.onAddServer();
    else if (props.onOpenSetup) props.onOpenSetup(item.target);
    else props.onRunBackup();
  };
  return (
    <button className="readiness-tile" type="button" data-ready={ready || undefined} data-tip={item.detail} data-tip-side="start" onClick={open}>
      <span className="readiness-tile__icon"><Icon size={18} aria-hidden="true" /></span>
      <strong>{item.label}</strong>
      {ready ? <CircleCheck size={16} aria-hidden="true" /> : <CircleAlert size={16} aria-hidden="true" />}
    </button>
  );
}

function ActionTile({ icon: Icon, label, onClick, primary }: { icon: LucideIcon; label: string; onClick: () => void; primary?: boolean }) {
  return (
    <button className="action-tile" type="button" data-primary={primary || undefined} onClick={onClick}>
      <Icon size={26} aria-hidden="true" />
      <strong>{label}</strong>
    </button>
  );
}
