import { useState, type ReactNode } from "react";
import { Check, LoaderCircle, X } from "lucide-react";
import type { Translate } from "../../i18n";
import { tip } from "../../shared/tip";
import { ExplorerTree } from "../explorer/ExplorerTree";
import { OperationFailureNotice } from "../OperationFailureNotice";
import { ResourceLoadFailure } from "../ResourceLoadFailure";
import { BackupToolbar } from "./BackupToolbar";
import { ReviewPane } from "./ReviewPane";
import { SelectionBasket } from "./SelectionBasket";
import { useBackupItems } from "./useBackupItems";
import { useCaptureResources } from "./useCaptureResources";
import { useCaptureRun } from "./useCaptureRun";

interface BackupWorkspaceProps {
  onPlansChanged: () => void;
  resourcesRevision: number;
  /** Rendered at the end of the toolbar (the setup toggle); also shown while prerequisites are missing. */
  toolbarEnd?: ReactNode;
  notReady: ReactNode;
  t: Translate;
}

export function BackupWorkspace({ onPlansChanged, resourcesRevision, toolbarEnd, notReady, t }: BackupWorkspaceProps) {
  const resources = useCaptureResources(resourcesRevision, t);
  const selection = useBackupItems();
  const [sqlitePath, setSqlitePath] = useState("");
  const run = useCaptureRun({
    profileId: resources.profileId, repositoryId: resources.repositoryId, items: selection.items, sqlitePath: sqlitePath.trim() || undefined,
  }, onPlansChanged, t);
  const changeProfile = (id: string) => { resources.setProfileId(id); selection.clear(); };
  if (resources.loading) return <div className="workspace-state"><LoaderCircle className="spin" size={18} aria-label={t("readinessLoading")} /></div>;
  if (resources.failure) {
    return (
      <section className="backup-workspace">
        <div className="backup-toolbar"><span className="backup-toolbar__spacer" />{toolbarEnd}</div>
        <ResourceLoadFailure message={resources.failure} onRetry={resources.retry} retryLabel={t("readinessRefresh")} retrying={false} />
      </section>
    );
  }
  return (
    <section className="backup-workspace" data-ready={resources.ready || undefined}>
      <BackupToolbar {...resources} disabled={run.running} onProfile={changeProfile} onRepository={resources.setRepositoryId} t={t}>{toolbarEnd}</BackupToolbar>
      {resources.ready ? (
        <div className="backup-workspace__body" inert={run.running}>
          <ExplorerTree profileId={resources.profileId} items={selection.items} onTogglePath={selection.togglePath} onSetDocker={selection.setDocker} t={t} />
        </div>
      ) : notReady}
      {resources.ready && (run.preview
        ? <ReviewPane preview={run.preview} running={run.running} cancelling={run.cancelling} onBack={run.back} onCreate={() => void run.run()} onCancel={() => void run.cancel()} t={t} />
        : <SelectionBasket items={selection.items} sqlitePath={sqlitePath} reviewing={run.reviewing} onSqliteChange={setSqlitePath} onRemove={selection.remove} onClear={selection.clear} onReview={() => void run.review()} t={t} />)}
      <Feedback run={run} t={t} />
    </section>
  );
}

function Feedback({ run, t }: { run: ReturnType<typeof useCaptureRun>; t: Translate }) {
  if (!run.result && !run.failure) return null;
  return (
    <div className="backup-feedback">
      {run.result && <p className="backup-feedback__ok" role="status"><Check size={15} aria-hidden="true" />{run.result}<button className="icon-button" type="button" onClick={run.dismissResult} {...tip(t("dismiss"))}><X size={13} aria-hidden="true" /></button></p>}
      {run.failure && <OperationFailureNotice message={run.failure} safe="captureFailureSafe" changed="captureFailureChanged" t={t} />}
    </div>
  );
}
