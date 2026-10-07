import { useState } from "react";
import type { Translate } from "../../i18n";
import {
  cancelJob, executeDeploy, executeSourceReplacement, previewDeploy, previewSourceReplacement,
  type DeploymentPreview, type ReplacementResult,
} from "../../shared/commands";
import { newRunId } from "../../shared/run-id";
import { safeErrorText } from "../../shared/safe-error";
import type { RestoreMode } from "./useRestoreSelection";

export type RestorePlan = { mode: "separate"; value: DeploymentPreview } | { mode: "replace"; value: ReplacementResult };

export interface ActionInput { repositoryId: string; backupId: string; profileId: string; mode: RestoreMode; targetPath: string }

const baseRequest = (input: ActionInput) => ({ repositoryId: input.repositoryId, backupId: input.backupId, targetProfileId: input.profileId });

/** Preview → typed confirmation → execute → cancel for one restore. */
export function useRestoreAction(t: Translate, input: ActionInput) {
  const [plan, setPlan] = useState<RestorePlan>();
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [runId, setRunId] = useState<string>();
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const fail = (error: unknown) => setFailure(safeErrorText(error, t("restoreErrorFallback")));
  const preview = async () => {
    setBusy(true); setFailure(undefined); setResult(undefined);
    try {
      setPlan(input.mode === "replace"
        ? { mode: "replace", value: await previewSourceReplacement(baseRequest(input)) }
        : { mode: "separate", value: await previewDeploy({ ...baseRequest(input), targetPath: input.targetPath }) });
      setConfirmation("");
    } catch (error) { fail(error); } finally { setBusy(false); }
  };
  const execute = async () => {
    if (!plan) return;
    const id = newRunId();
    setRunId(id); setBusy(true); setFailure(undefined);
    try {
      const where = plan.mode === "replace"
        ? (await executeSourceReplacement({ ...baseRequest(input), confirmation, runId: id })).root
        : (await executeDeploy({ ...baseRequest(input), targetPath: input.targetPath, confirmation, runId: id })).targetPath;
      setResult(`${t("restoreSuccess")} ${where}`); setPlan(undefined); setConfirmation("");
    } catch (error) { fail(error); } finally { setBusy(false); setRunId(undefined); setCancelling(false); }
  };
  const cancel = async () => {
    if (!runId || cancelling) return;
    setCancelling(true);
    try { await cancelJob(runId); } catch (error) { fail(error); setCancelling(false); }
  };
  return { plan, confirmation, busy, cancelling, runId, result, failure, setConfirmation, preview, execute, cancel, discardPlan: () => setPlan(undefined), dismissResult: () => setResult(undefined) };
}
