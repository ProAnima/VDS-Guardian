import { ArrowLeft, Check, CircleAlert, Database, LoaderCircle, ShieldCheck } from "lucide-react";
import type { Translate } from "../../i18n";
import type { CaptureSelectionPreview, CaptureSelectionWarning } from "../../shared/commands";
import { tip } from "../../shared/tip";

interface ReviewPaneProps {
  preview: CaptureSelectionPreview;
  running: boolean;
  cancelling: boolean;
  onBack: () => void;
  onCreate: () => void;
  onCancel: () => void;
  t: Translate;
}

export function ReviewPane({ preview, running, cancelling, onBack, onCreate, onCancel, t }: ReviewPaneProps) {
  return (
    <aside className="basket review" aria-label={t("captureReviewTitle")}>
      <header className="basket__header">
        <button className="icon-button" type="button" disabled={running} onClick={onBack} {...tip(t("reviewBack"))}><ArrowLeft size={15} aria-hidden="true" /></button>
        <ShieldCheck size={16} aria-hidden="true" data-tip={t("captureReviewSafe")} />
        <strong>{t("captureReviewTitle")}</strong>
      </header>
      <div className="review__body">
        <ul className="review__paths" aria-label={t("captureReviewPaths")}>
          {preview.normalizedRoots.map((root) => <li key={root}><code>{root}</code></li>)}
        </ul>
        {preview.sqlitePath && <p className="review__sqlite" data-tip={t("captureReviewSqlite")}><Database size={14} aria-hidden="true" /><code>{preview.sqlitePath}</code></p>}
        {preview.warnings.map((warning, index) => (
          <p className="review__warning" key={`${warning.kind}-${index}`}><CircleAlert size={14} aria-hidden="true" />{warningText(warning, t)}</p>
        ))}
      </div>
      {running
        ? <RunningBar cancelling={cancelling} onCancel={onCancel} t={t} />
        : <button className="button button--primary basket__go" type="button" onClick={onCreate}><Check size={16} aria-hidden="true" />{t("backupCreate")}</button>}
    </aside>
  );
}

function RunningBar({ cancelling, onCancel, t }: { cancelling: boolean; onCancel: () => void; t: Translate }) {
  return (
    <div className="review__running" role="status">
      <LoaderCircle className="spin" size={16} aria-hidden="true" /><span>{t("backupCreating")}</span>
      <button className="button button--secondary" type="button" disabled={cancelling} onClick={onCancel}>{t("captureCancel")}</button>
    </div>
  );
}

function warningText(warning: CaptureSelectionWarning, t: Translate): string {
  if (warning.kind === "covered_path") return `${warning.path} ${t("captureWarningCovered")} ${warning.coveredBy}`;
  if (warning.kind === "live_docker_data") return `${warning.containerName}: ${t("captureWarningLiveDocker")}`;
  return `${warning.sqlitePath}: ${t("captureWarningSqliteCovered")} ${warning.coveredBy}`;
}
