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

const sameInput = (left: ActionInput, right: ActionInput) =>
  left.repositoryId === right.repositoryId && left.backupId === right.backupId && left.profileId === right.profileId
  && left.mode === right.mode && left.targetPath === right.targetPath;

const baseRequest = (input: ActionInput) => ({ repositoryId: input.repositoryId, backupId: input.backupId, targetProfileId: input.profileId });

/** The run id of an in-flight restore and a single, idempotent cancellation request for it. */
function useRunCancellation(onError: (error: unknown) => void) {
  const [runId, setRunId] = useState<string>();
  const [cancelling, setCancelling] = useState(false);
  const cancel = async () => {
    if (!runId || cancelling) return;
    setCancelling(true);
    try { await cancelJob(runId); } catch (error) { onError(error); setCancelling(false); }
  };
  const begin = () => { const id = newRunId(); setRunId(id); return id; };
  const end = () => { setRunId(undefined); setCancelling(false); };
  return { runId, cancelling, cancel, begin, end };
}

/** Preview → typed confirmation → execute → cancel for one restore. */
export function useRestoreAction(t: Translate, input: ActionInput) {
  // A plan confirms exactly the backup, server, mode and path it was previewed for; any later
  // change makes it stale, and execution always uses the inputs the plan was made from.
  const [previewed, setPreviewed] = useState<{ plan: RestorePlan; input: ActionInput }>();
  const plan = previewed && sameInput(previewed.input, input) ? previewed.plan : undefined;
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const fail = (error: unknown) => setFailure(safeErrorText(error, t("restoreErrorFallback")));
  const run = useRunCancellation(fail);
  const preview = async () => {
    setBusy(true); setFailure(undefined); setResult(undefined);
    try {
      const snapshot = { ...input };
      setPreviewed({
        input: snapshot,
        plan: snapshot.mode === "replace"
          ? { mode: "replace", value: await previewSourceReplacement(baseRequest(snapshot)) }
          : { mode: "separate", value: await previewDeploy({ ...baseRequest(snapshot), targetPath: snapshot.targetPath }) },
      });
      setConfirmation("");
    } catch (error) { fail(error); } finally { setBusy(false); }
  };
  const execute = async () => {
    if (!plan || !previewed) return;
    const source = previewed.input;
    const id = run.begin();
    setBusy(true); setFailure(undefined);
    try {
      const where = plan.mode === "replace"
        ? (await executeSourceReplacement({ ...baseRequest(source), confirmation, runId: id })).root
        : (await executeDeploy({ ...baseRequest(source), targetPath: source.targetPath, confirmation, runId: id })).targetPath;
      setResult(`${t("restoreSuccess")} ${where}`); setPreviewed(undefined); setConfirmation("");
    } catch (error) { fail(error); } finally { setBusy(false); run.end(); }
  };
  return {
    plan, confirmation, busy, cancelling: run.cancelling, runId: run.runId, result, failure, setConfirmation, preview, execute,
    cancel: run.cancel, discardPlan: () => setPreviewed(undefined), dismissResult: () => setResult(undefined),
  };
}
