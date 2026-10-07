import { ArrowLeft, CircleAlert, LoaderCircle, RotateCcw } from "lucide-react";
import type { Translate } from "../../i18n";
import { tip } from "../../shared/tip";
import type { RestorePlan } from "./useRestoreAction";
import type { RestoreModel } from "./useRestoreModel";

export function PlanConfirm({ model, plan, t }: { model: RestoreModel; plan: RestorePlan; t: Translate }) {
  const { action } = model;
  const phrase = plan.value.confirmation;
  const replace = plan.mode === "replace";
  const conflicts = replace ? plan.value.conflicts : [];
  const blocked = conflicts.length > 0;
  return (
    <section className="confirm" aria-label={t("restorePlanTitle")}>
      <div className="confirm__facts">
        <p data-tip={t("restorePlanDestination")}><RotateCcw size={14} aria-hidden="true" /><code>{replace ? plan.value.root : plan.value.targetPath}</code></p>
        {replace && plan.value.replaces.map((path) => <p key={path} data-tip={t("restoreImpactReplaces")}><CircleAlert size={14} aria-hidden="true" /><code>{path}</code></p>)}
        {replace && plan.value.containers.length > 0 && <p data-tip={t("restoreImpactWorkloads")}>{plan.value.containers.join(", ")}</p>}
        {replace && <p className="confirm__rollback"><CircleAlert size={14} aria-hidden="true" />{t("restorePlanRollback")}</p>}
        {conflicts.map((item) => <p className="confirm__conflict" key={item}><CircleAlert size={14} aria-hidden="true" />{conflictText(item, t)}</p>)}
      </div>
      <code className="confirm__phrase" data-tip={t("restorePlanConfirmLabel")}>{phrase}</code>
      <div className="confirm__actions">
        <button className="icon-button icon-button--large" type="button" disabled={action.busy} onClick={action.discardPlan} {...tip(t("restoreCancel"))}><ArrowLeft size={16} aria-hidden="true" /></button>
        <input value={action.confirmation} onChange={(event) => action.setConfirmation(event.target.value)} placeholder={t("restoreConfirmPlaceholder")} aria-label={t("restorePlanConfirmLabel")} spellCheck={false} autoComplete="off" />
        {action.runId && <button className="button button--secondary" type="button" disabled={action.cancelling} onClick={() => void action.cancel()}>{action.cancelling && <LoaderCircle className="spin" size={15} aria-hidden="true" />}{t("restoreCancelRunning")}</button>}
        <button className="button button--primary" type="button" disabled={action.busy || action.confirmation !== phrase || blocked} onClick={() => void action.execute()}>
          {action.busy ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <RotateCcw size={16} aria-hidden="true" />}
          {action.busy ? t("restoreExecuting") : t("restoreExecute")}
        </button>
      </div>
    </section>
  );
}

function conflictText(value: string, t: Translate): string {
  const detail = value.split(":", 2)[1];
  return detail ? `${t("restoreFailureChanged")}: ${detail}` : t("restoreFailureChanged");
}
