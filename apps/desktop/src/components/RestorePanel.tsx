import { Archive, Check, LoaderCircle, ServerOff, X } from "lucide-react";
import type { ReactNode } from "react";
import type { Translate } from "../i18n";
import { tip } from "../shared/tip";
import { EmptyNotice } from "./EmptyNotice";
import { OperationFailureNotice } from "./OperationFailureNotice";
import { ResourceLoadFailure } from "./ResourceLoadFailure";
import { BackupContents } from "./restore/BackupContents";
import { BackupList } from "./restore/BackupList";
import { DestinationBar } from "./restore/DestinationBar";
import { PlanConfirm } from "./restore/PlanConfirm";
import { useRestoreModel, type RestoreModel } from "./restore/useRestoreModel";

interface RestorePanelProps { onManageBackups?: () => void; onManageServers?: () => void; t: Translate }

export function RestorePanel({ onManageBackups, onManageServers, t }: RestorePanelProps) {
  const model = useRestoreModel(t);
  const { resources } = model;
  if (resources.loading) return <main className="view"><div className="workspace-state"><LoaderCircle className="spin" size={18} aria-label={t("readinessLoading")} /></div></main>;
  if (resources.failure) return <main className="view"><ResourceLoadFailure message={resources.failure} onRetry={resources.retry} retryLabel={t("readinessRefresh")} retrying={false} /></main>;
  if (resources.repositories.length === 0) return <main className="view"><EmptyNotice icon={Archive} message={t("restoreNoRepositories")} action={t("navBackups")} onAction={onManageBackups} /></main>;
  return (
    <main className="view">
      <section className="restore-workspace">
        <div className="backup-toolbar">
          <label className="picker" data-tip={t("restoreRepository")}>
            <Archive size={15} aria-hidden="true" />
            <select value={resources.repositoryId} aria-label={t("restoreRepository")} disabled={model.action.busy} onChange={(event) => resources.setRepositoryId(event.target.value)}>
              {resources.repositories.map((item) => <option key={item.repositoryId} value={item.repositoryId}>{item.label}</option>)}
            </select>
          </label>
        </div>
        <aside className="restore-workspace__list"><BackupsColumn model={model} t={t} /></aside>
        <div className="restore-workspace__detail"><DetailColumn model={model} t={t} onManageServers={onManageServers} /></div>
        <Feedback model={model} t={t} />
      </section>
    </main>
  );
}

function Loading({ label }: { label: string }): ReactNode {
  return <div className="workspace-state"><LoaderCircle className="spin" size={18} aria-label={label} /></div>;
}

function BackupsColumn({ model, t }: { model: RestoreModel; t: Translate }) {
  const { backups } = model;
  if (backups.loading) return <Loading label={t("restorePreviewing")} />;
  if (backups.failure) return <ResourceLoadFailure message={backups.failure} onRetry={backups.retry} retryLabel={t("readinessRefresh")} retrying={false} />;
  if (backups.backups.length === 0) return <EmptyNotice icon={Archive} message={t("restoreNoBackups")} />;
  return <BackupList backups={backups.backups} selectedId={backups.backupId} onSelect={backups.setBackupId} t={t} />;
}

function DetailColumn({ model, t, onManageServers }: { model: RestoreModel; t: Translate; onManageServers?: () => void }) {
  const { selection, action, resources } = model;
  if (selection.loading) return <Loading label={t("restorePreviewing")} />;
  if (selection.failure) return <ResourceLoadFailure message={selection.failure} onRetry={selection.retry} retryLabel={t("readinessRefresh")} retrying={false} />;
  if (!selection.description) return null;
  return (
    <>
      <BackupContents description={selection.description} t={t} />
      {resources.profiles.length === 0
        ? <EmptyNotice icon={ServerOff} message={t("deployNoTargetProfiles")} action={t("addServer")} onAction={onManageServers} />
        : action.plan ? <PlanConfirm model={model} plan={action.plan} t={t} /> : <DestinationBar model={model} t={t} />}
    </>
  );
}

function Feedback({ model, t }: { model: RestoreModel; t: Translate }) {
  const { action } = model;
  if (!action.result && !action.failure) return null;
  return (
    <div className="backup-feedback restore-workspace__feedback">
      {action.result && <p className="backup-feedback__ok" role="status"><Check size={15} aria-hidden="true" />{action.result}<button className="icon-button" type="button" onClick={action.dismissResult} {...tip(t("dismiss"))}><X size={13} aria-hidden="true" /></button></p>}
      {action.failure && <OperationFailureNotice message={action.failure} safe="restoreFailureSafe" changed="restoreFailureChanged" t={t} />}
    </div>
  );
}
