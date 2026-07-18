import { Check, CircleAlert, LoaderCircle, ShieldCheck } from "lucide-react";
import type { Translate } from "../i18n";
import type { CaptureSelectionPreview, CaptureSelectionWarning } from "../shared/commands";

interface CaptureSelectionReviewProps {
  preview: CaptureSelectionPreview;
  saving: boolean;
  onSave: () => void;
  t: Translate;
}

export function CaptureSelectionReview({ preview, saving, onSave, t }: CaptureSelectionReviewProps) {
  return <section className="capture-review" aria-labelledby="capture-review-title">
    <header><div><ShieldCheck size={18} /><strong id="capture-review-title">{t("captureReviewTitle")}</strong></div><p>{t("captureReviewSafe")}</p></header>
    <div className="capture-review__roots"><span>{t("captureReviewPaths")}</span>{preview.normalizedRoots.map((root) => <code key={root}>{root}</code>)}</div>
    {preview.sqlitePath && <p><strong>{t("captureReviewSqlite")}</strong> <code>{preview.sqlitePath}</code></p>}
    {preview.warnings.length > 0 && <div className="capture-review__warnings"><strong><CircleAlert size={15} />{t("captureReviewWarnings")}</strong>{preview.warnings.map((warning, index) => <p key={`${warning.kind}-${index}`}>{warningText(warning, t)}</p>)}</div>}
    <p className="capture-review__phrase"><span>{t("captureReviewConfirmation")}</span><code>{preview.confirmation}</code></p>
    <button className="button button--primary" disabled={saving} type="button" onClick={onSave}>{saving ? <LoaderCircle className="spin" size={16} /> : <Check size={16} />}{saving ? t("backupCreating") : t("backupCreate")}</button>
  </section>;
}

function warningText(warning: CaptureSelectionWarning, t: Translate): string {
  if (warning.kind === "covered_path") return `${warning.path} ${t("captureWarningCovered")} ${warning.coveredBy}`;
  if (warning.kind === "live_docker_data") return `${warning.containerName}: ${t("captureWarningLiveDocker")}`;
  return `${warning.sqlitePath}: ${t("captureWarningSqliteCovered")} ${warning.coveredBy}`;
}
