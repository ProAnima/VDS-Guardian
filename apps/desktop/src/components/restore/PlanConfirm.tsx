import { ArrowLeft, CircleAlert, LoaderCircle, RotateCcw } from "lucide-react";
import type { ReactNode } from "react";
import type { Translate } from "../../i18n";
import { tip } from "../../shared/tip";
import type { RestorePlan } from "./useRestoreAction";
import type { RestoreModel } from "./useRestoreModel";

/**
 * The last step before a restore. Everything the operator agrees to is spelled out as visible
 * label/value pairs — the server, the destination, what gets replaced, which services stop, the
 * safety backup and the rollback copy — not hidden in tooltips.
 */
export function PlanConfirm({ model, plan, t }: { model: RestoreModel; plan: RestorePlan; t: Translate }) {
  const { action } = model;
  const phrase = plan.value.confirmation;
  const conflicts = plan.mode === "replace" ? plan.value.conflicts : [];
  const server = model.resources.profiles.find((profile) => profile.profileId === plan.value.targetProfileId)?.label ?? plan.value.targetProfileId;
  return (
    <section className="confirm" aria-label={t("restorePlanTitle")}>
      <dl className="confirm__facts">
        <Fact label={t("deployTargetProfile")}>{server}</Fact>
        {plan.mode === "separate" ? <Fact label={t("restorePlanDestination")}><code>{plan.value.targetPath}</code></Fact> : <ReplaceFacts plan={plan.value} t={t} />}
      </dl>
      {plan.mode === "replace" && <p className="confirm__rollback"><CircleAlert size={14} aria-hidden="true" />{t("restorePlanRollback")}</p>}
      {conflicts.map((item) => <p className="confirm__conflict" key={item}><CircleAlert size={14} aria-hidden="true" />{conflictText(item, t)}</p>)}
      <code className="confirm__phrase" data-tip={t("restorePlanConfirmLabel")}>{phrase}</code>
      <div className="confirm__actions">
        <button className="icon-button icon-button--large" type="button" disabled={action.busy} onClick={action.discardPlan} {...tip(t("restoreCancel"))}><ArrowLeft size={16} aria-hidden="true" /></button>
        <input value={action.confirmation} onChange={(event) => action.setConfirmation(event.target.value)} placeholder={t("restoreConfirmPlaceholder")} aria-label={t("restorePlanConfirmLabel")} spellCheck={false} autoComplete="off" />
        {action.runId && <button className="button button--secondary" type="button" disabled={action.cancelling} onClick={() => void action.cancel()}>{action.cancelling && <LoaderCircle className="spin" size={15} aria-hidden="true" />}{t("restoreCancelRunning")}</button>}
        <button className="button button--primary" type="button" disabled={action.busy || action.confirmation !== phrase || conflicts.length > 0} onClick={() => void action.execute()}>
          {action.busy ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <RotateCcw size={16} aria-hidden="true" />}
          {action.busy ? t("restoreExecuting") : t("restoreExecute")}
        </button>
      </div>
    </section>
  );
}

function ReplaceFacts({ plan, t }: { plan: Extract<RestorePlan, { mode: "replace" }>["value"]; t: Translate }) {
  return (
    <>
      <Fact label={t("restorePlanDestination")}><code>{plan.root}</code></Fact>
      {plan.replaces.length > 0 && <Fact label={t("restoreImpactReplaces")} tone="warn">{plan.replaces.map((path) => <code key={path}>{path}</code>)}</Fact>}
      {plan.serviceStopRequired && plan.containers.length > 0 && <Fact label={t("restorePlanServiceStop")} tone="warn">{plan.containers.join(", ")}</Fact>}
      {plan.safetyBackupRequired && <Fact label={t("restorePlanSafetyBackup")}>{plan.safetyBackupId ? <code>{plan.safetyBackupId}</code> : t("restorePlanSafetyBackupAuto")}</Fact>}
      {plan.rollbackPath && <Fact label={t("restorePlanRollbackPath")}><code>{plan.rollbackPath}</code></Fact>}
    </>
  );
}

function Fact({ label, tone, children }: { label: string; tone?: "warn"; children: ReactNode }) {
  return <div className="confirm__fact" data-tone={tone}><dt>{label}</dt><dd>{children}</dd></div>;
}

/** Conflicts arrive as `<code>:<detail>`; the detail itself may contain colons (paths, times). */
function conflictText(value: string, t: Translate): string {
  const separator = value.indexOf(":");
  const detail = separator < 0 ? "" : value.slice(separator + 1).trim();
  return detail ? `${t("restoreFailureChanged")}: ${detail}` : t("restoreFailureChanged");
}
