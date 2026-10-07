import { useState } from "react";
import type { Translate } from "../../i18n";
import { captureErrorText } from "../../shared/capture-error";
import {
  cancelJob, hasTauriRuntime, previewCaptureSelection, runCaptureSelection,
  type BackupSelection, type CaptureSelectionPreview,
} from "../../shared/commands";
import { newRunId } from "../../shared/run-id";
import { safeErrorText } from "../../shared/safe-error";

/** Preview → confirm → run → cancel lifecycle for one capture selection. */
export function useCaptureRun(selection: BackupSelection, onSealed: () => void, t: Translate) {
  const key = JSON.stringify(selection);
  const [reviewed, setReviewed] = useState<{ key: string; preview: CaptureSelectionPreview }>();
  // A preview only confirms the exact selection it was made for; any later change makes it stale.
  const preview = reviewed?.key === key ? reviewed.preview : undefined;
  const [reviewing, setReviewing] = useState(false);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [runId, setRunId] = useState<string>();
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const back = () => setReviewed(undefined);
  const review = async () => {
    if (!hasTauriRuntime()) return;
    setReviewing(true); setFailure(undefined); setResult(undefined);
    try { setReviewed({ key, preview: await previewCaptureSelection(selection) }); }
    catch { setFailure(t("captureReviewFailed")); }
    finally { setReviewing(false); }
  };
  const run = async () => {
    if (!preview) return;
    const nextRunId = newRunId();
    setRunId(nextRunId); setRunning(true); setFailure(undefined);
    try {
      const job = await runCaptureSelection({ selection, confirmation: preview.confirmation, runId: nextRunId });
      onSealed(); setReviewed(undefined); setResult(`${t("captureSealed")} ${job.backupId}`);
    } catch (error) { setFailure(captureErrorText(error, t("captureErrorFallback"))); }
    finally { setRunning(false); setRunId(undefined); setCancelling(false); }
  };
  const cancel = async () => {
    if (!runId || cancelling) return;
    setCancelling(true);
    try { await cancelJob(runId); }
    catch (error) { setFailure(safeErrorText(error, t("captureErrorFallback"))); setCancelling(false); }
  };
  return { preview, reviewing, running, cancelling, result, failure, back, review, run, cancel, dismissResult: () => setResult(undefined) };
}

export type CaptureRun = ReturnType<typeof useCaptureRun>;
