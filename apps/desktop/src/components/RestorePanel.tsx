import { useEffect, useState, type FormEvent } from "react";
import { Box, Check, Eye, File, Folder, LoaderCircle, RotateCcw, Server } from "lucide-react";
import type { Translate } from "../i18n";
import { OperationFailureNotice } from "./OperationFailureNotice";
import { safeErrorText } from "../shared/safe-error";
import {
  cancelJob, executeDeploy, executeSourceReplacement, hasTauriRuntime, inspectRestoreBackup,
  listBackups, listRepositories, listSshProfiles, previewDeploy, previewSourceReplacement,
  type BackupRestoreDescription, type BackupSummary, type DeploymentPreview,
  type ReplacementResult, type RepositorySummary, type SshProfileSummary,
} from "../shared/commands";
import { newRunId } from "../shared/run-id";
import { ResourceLoadFailure } from "./ResourceLoadFailure";

interface RestorePanelProps { onManageBackups?: () => void; onManageServers?: () => void; t: Translate; }
type RestoreMode = "separate" | "replace";
type Plan = { mode: "separate"; value: DeploymentPreview } | { mode: "replace"; value: ReplacementResult };

export function RestorePanel({ onManageBackups, onManageServers, t }: RestorePanelProps) {
  const model = useRestoreModel(t);
  return <main className="dashboard">
    <section className="hero-panel backup-hero"><div className="hero-panel__content">
      <p className="eyebrow"><RotateCcw size={15} />{t("restoreEyebrow")}</p>
      <h1>{t("restoreTitle")}</h1><p>{t("restoreBody")}</p>
    </div></section>
    <section className="repository-panel" aria-labelledby="restore-title">
      <header className="repository-panel__header"><h2 id="restore-title">{t("restoreBackupsTitle")}</h2></header>
      {model.plan ? <Confirmation model={model} t={t} /> : <RestoreForm model={model} onManageBackups={onManageBackups} onManageServers={onManageServers} t={t} />}
      {model.result && <p className="repository-panel__success" role="status"><Check size={16} />{model.result}</p>}
      {model.failure && <OperationFailureNotice message={model.failure} safe="restoreFailureSafe" changed="restoreFailureChanged" t={t} />}
      {!hasTauriRuntime() && <p className="signing-panel__desktop">{t("restoreDesktopRequired")}</p>}
    </section>
  </main>;
}

function useRestoreModel(t: Translate) {
  const resources = useRestoreResources(t);
  const backups = useRestoreBackups(resources.repositoryId, t);
  const selection = useRestoreSelection(resources.profiles, resources.repositoryId, backups.backupId, t);
  const { repositoryId } = resources; const { backupId } = backups;
  const { profileId, mode, targetPath } = selection;
  const action = useRestoreAction(t, { repositoryId, backupId, profileId, mode, targetPath });
  return { ...resources, ...backups, ...selection, ...action };
}

function useRestoreResources(t: Translate) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]); const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [repositoryId, setRepositoryId] = useState(""); const [resourcesLoading, setResourcesLoading] = useState(true);
  const [resourcesFailure, setResourcesFailure] = useState<string>(); const [retryRevision, setRetryRevision] = useState(0);
  useEffect(() => { let active = true; setResourcesLoading(true); setResourcesFailure(undefined); void Promise.all([listRepositories(), listSshProfiles()]).then(([repos, servers]) => {
    if (!active) return; setRepositories(repos); setProfiles(servers); setRepositoryId((current) => retainedResourceId(current, repos, "repositoryId"));
  }).catch((error: unknown) => { if (active) setResourcesFailure(safeErrorText(error, t("restoreErrorFallback"))); })
    .finally(() => { if (active) setResourcesLoading(false); }); return () => { active = false; }; }, [retryRevision, t]);
  return { repositories, profiles, repositoryId, setRepositoryId, resourcesLoading, resourcesFailure, retryResources: () => setRetryRevision((current) => current + 1) };
}

function useRestoreBackups(repositoryId: string, t: Translate) {
  const [backups, setBackups] = useState<BackupSummary[]>([]); const [backupId, setBackupId] = useState("");
  const [backupsLoading, setBackupsLoading] = useState(false); const [backupsFailure, setBackupsFailure] = useState<string>(); const [retryRevision, setRetryRevision] = useState(0);
  useEffect(() => { setBackups([]); setBackupId(""); setBackupsFailure(undefined); if (!repositoryId) return;
    let active = true; setBackupsLoading(true); void listBackups(repositoryId).then((items) => { if (!active) return; setBackups(items); setBackupId(items[0]?.backupId ?? ""); })
      .catch((error: unknown) => { if (active) setBackupsFailure(safeErrorText(error, t("restoreErrorFallback"))); })
      .finally(() => { if (active) setBackupsLoading(false); }); return () => { active = false; };
  }, [repositoryId, retryRevision, t]);
  return { backups, backupId, setBackupId, backupsLoading, backupsFailure, retryBackups: () => setRetryRevision((current) => current + 1) };
}

function useRestoreSelection(profiles: SshProfileSummary[], repositoryId: string, backupId: string, t: Translate) {
  const [description, setDescription] = useState<BackupRestoreDescription>(); const [profileId, setProfileId] = useState("");
  const [mode, setMode] = useState<RestoreMode>("separate"); const [separatePath, setSeparatePath] = useState("");
  const [descriptionLoading, setDescriptionLoading] = useState(false); const [descriptionFailure, setDescriptionFailure] = useState<string>(); const [retryRevision, setRetryRevision] = useState(0);
  useEffect(() => { setDescription(undefined); setDescriptionFailure(undefined); if (!repositoryId || !backupId) return;
    let active = true; setDescriptionLoading(true); void inspectRestoreBackup(repositoryId, backupId).then((value) => { if (!active) return;
      setDescription(value); setProfileId(preferredProfileId(value.sourceProfileId, profiles)); setSeparatePath(suggestRestorePath(value.roots[0]));
      if (!value.replacementAvailable || !profiles.some((item) => item.profileId === value.sourceProfileId)) setMode("separate");
    }).catch((error: unknown) => { if (active) setDescriptionFailure(safeErrorText(error, t("restoreErrorFallback"))); })
      .finally(() => { if (active) setDescriptionLoading(false); }); return () => { active = false; };
  }, [repositoryId, backupId, profiles, retryRevision, t]);
  const targetPath = mode === "replace" ? description?.roots[0] ?? "" : separatePath;
  return { description, profileId, setProfileId, mode, setMode, targetPath, setTargetPath: setSeparatePath, descriptionLoading, descriptionFailure, retryDescription: () => setRetryRevision((current) => current + 1) };
}

interface ActionInput { repositoryId: string; backupId: string; profileId: string; mode: RestoreMode; targetPath: string; }
function useRestoreAction(t: Translate, input: ActionInput) {
  const [plan, setPlan] = useState<Plan>(); const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false); const [cancelling, setCancelling] = useState(false); const [runId, setRunId] = useState<string>();
  const [result, setResult] = useState<string>(); const [failure, setFailure] = useState<string>();
  const preview = (event: FormEvent) => { event.preventDefault(); void runPreview(); };
  const runPreview = async () => { setBusy(true); setFailure(undefined); setResult(undefined); try {
    if (input.mode === "replace") setPlan({ mode: "replace", value: await previewSourceReplacement(request(input)) });
    else setPlan({ mode: "separate", value: await previewDeploy({ ...request(input), targetPath: input.targetPath }) });
    setConfirmation("");
  } catch (error) { setFailure(safeErrorText(error, t("restoreErrorFallback"))); } finally { setBusy(false); } };
  const execute = async () => { if (!plan) return; const id = newRunId(); setRunId(id); setBusy(true); setFailure(undefined); try {
    if (plan.mode === "replace") { const done = await executeSourceReplacement({ ...request(input), confirmation, runId: id }); setResult(`${t("restoreSuccess")} ${done.root}`); }
    else { const done = await executeDeploy({ ...request(input), targetPath: input.targetPath, confirmation, runId: id }); setResult(`${t("restoreSuccess")} ${done.targetPath}`); }
    setPlan(undefined); setConfirmation("");
  } catch (error) { setFailure(safeErrorText(error, t("restoreErrorFallback"))); } finally { setBusy(false); setRunId(undefined); setCancelling(false); } };
  const cancel = async () => { if (!runId || cancelling) return; setCancelling(true); try { await cancelJob(runId); } catch (error) { setFailure(safeErrorText(error, t("restoreErrorFallback"))); setCancelling(false); } };
  return { plan, setPlan, confirmation, setConfirmation, busy, cancelling, runId, result, failure, setFailure, preview, execute, cancel };
}

function request(input: ActionInput) { return { repositoryId: input.repositoryId, backupId: input.backupId, targetProfileId: input.profileId }; }
type Model = ReturnType<typeof useRestoreModel>;

function RestoreForm({ model, onManageBackups, onManageServers, t }: { model: Model; onManageBackups?: () => void; onManageServers?: () => void; t: Translate }) {
  if (model.resourcesLoading) return <p className="restore-panel__empty">{t("readinessLoading")}</p>;
  if (model.resourcesFailure) return <ResourceLoadFailure message={model.resourcesFailure} onRetry={model.retryResources} retryLabel={t("readinessRefresh")} retrying={model.resourcesLoading} />;
  if (model.repositories.length === 0) return <EmptyRestoreState action={t("navBackups")} message={t("restoreNoRepositories")} onClick={onManageBackups} />;
  return <form className="repository-form" onSubmit={model.preview}>
    <label><span>{t("restoreRepository")}</span><select value={model.repositoryId} onChange={(e) => model.setRepositoryId(e.target.value)}>{model.repositories.map((item) => <option key={item.repositoryId} value={item.repositoryId}>{item.label}</option>)}</select></label>
    <label><span>{t("restoreBackupsTitle")}</span><select value={model.backupId} disabled={model.backupsLoading || model.backups.length === 0} onChange={(e) => model.setBackupId(e.target.value)}>{model.backups.map((item) => <option key={item.backupId} value={item.backupId}>{item.backupId} — {item.sealedAt}</option>)}</select></label>
    {model.backupsLoading && <p className="restore-panel__empty repository-form__wide">{t("restorePreviewing")}</p>}
    {!model.backupsLoading && model.backupsFailure && <div className="repository-form__wide"><ResourceLoadFailure message={model.backupsFailure} onRetry={model.retryBackups} retryLabel={t("readinessRefresh")} retrying={model.backupsLoading} /></div>}
    {!model.backupsLoading && !model.backupsFailure && model.backups.length === 0 && <p className="restore-panel__empty repository-form__wide">{t("restoreNoBackups")}</p>}
    {model.descriptionLoading && <p className="restore-panel__empty repository-form__wide">{t("restorePreviewing")}</p>}
    {!model.descriptionLoading && model.descriptionFailure && <div className="repository-form__wide"><ResourceLoadFailure message={model.descriptionFailure} onRetry={model.retryDescription} retryLabel={t("readinessRefresh")} retrying={model.descriptionLoading} /></div>}
    {model.description && <>
      <BackupExplorer description={model.description} t={t} />
      {model.profiles.length === 0
        ? <EmptyRestoreState action={t("addServer")} message={t("deployNoTargetProfiles")} onClick={onManageServers} />
        : <>
      <div className="restore-mode" role="radiogroup" aria-label={t("restorePlanDestination")}>
        <button data-active={model.mode === "separate" || undefined} role="radio" aria-checked={model.mode === "separate"} type="button" onClick={() => model.setMode("separate")}><Folder size={17} />{t("restoreModeSeparate")}</button>
        <button data-active={model.mode === "replace" || undefined} role="radio" aria-checked={model.mode === "replace"} type="button" disabled={!replacementReady(model)} onClick={() => model.setMode("replace")}><RotateCcw size={17} />{t("restoreModeReplace")}</button>
      </div>
      <p className="restore-mode__hint">{t(model.mode === "replace" ? "restorePlanRollback" : "restoreDestinationHint")}</p>
      <label><span>{t("deployTargetProfile")}</span><select value={model.profileId} disabled={model.mode === "replace"} onChange={(e) => model.setProfileId(e.target.value)}>{model.profiles.map((item) => <option key={item.profileId} value={item.profileId}>{item.label}</option>)}</select></label>
      <label><span>{t("deployTargetPath")}</span><input value={model.targetPath} readOnly={model.mode === "replace"} onChange={(e) => model.setTargetPath(e.target.value)} placeholder={t("deployTargetPathHint")} /></label>
      <button className="button button--primary" disabled={model.busy || !model.profileId || !model.targetPath} type="submit">{model.busy ? <LoaderCircle className="spin" size={16} /> : <Eye size={16} />}{model.busy ? t("restorePreviewing") : t("restorePreview")}</button>
        </>}
    </>}
  </form>;
}

function EmptyRestoreState({ action, message, onClick }: { action: string; message: string; onClick?: () => void }) {
  return <div className="restore-panel__empty-action"><p>{message}</p><button className="button button--secondary" disabled={!onClick} onClick={onClick} type="button">{action}</button></div>;
}

function BackupExplorer({ description, t }: { description: BackupRestoreDescription; t: Translate }) {
  return <div className="restore-explorer">
    <div><strong><Server size={15} /> {t("restorePlanSource")}</strong>{description.roots.map((root) => <code key={root}>{root}</code>)}</div>
    <div><strong><File size={15} /> {t("restoreImpactAdds")} ({description.entries.length}/{description.totalEntries})</strong><ul>{description.entries.map((entry) => <li key={entry.path}>{entry.kind === "directory" ? <Folder size={14} /> : <File size={14} />}<code>{entry.path}</code></li>)}</ul></div>
    {description.dockerWorkloads.length > 0 && <div><strong><Box size={15} /> {t("restoreImpactWorkloads")}</strong><ul>{description.dockerWorkloads.map((item) => <li key={item.containerId}><code>{item.containerName}</code> · {item.image} · {t(activeState(item.state) ? "dockerActive" : "dockerStopped")}</li>)}</ul></div>}
  </div>;
}

function activeState(state: string): boolean { return ["running", "paused", "restarting"].includes(state); }

function replacementReady(model: Model): boolean { return Boolean(model.description?.replacementAvailable && model.profiles.some((item) => item.profileId === model.description?.sourceProfileId)); }

function preferredProfileId(sourceProfileId: string, profiles: SshProfileSummary[]): string { return profiles.some((item) => item.profileId === sourceProfileId) ? sourceProfileId : profiles[0]?.profileId ?? ""; }

function suggestRestorePath(root: string | undefined): string { if (!root || root === "/") return "/restore"; return `${root.replace(/\/$/u, "")}-restored`; }

function retainedResourceId<T extends Record<K, string>, K extends "repositoryId">(current: string, items: T[], key: K): string { return items.some((item) => item[key] === current) ? current : items[0]?.[key] ?? ""; }

function Confirmation({ model, t }: { model: Model; t: Translate }) {
  const plan = model.plan; if (!plan) return null;
  const phrase = plan.value.confirmation;
  const destination = plan.mode === "replace" ? plan.value.root : plan.value.targetPath;
  const replaces = plan.mode === "replace" ? plan.value.replaces : [];
  return <div className="signing-confirm"><strong>{t("restorePlanTitle")}</strong>
    <p>{t("restorePlanDestination")}: <code>{destination}</code></p>
    {replaces.map((path) => <p key={path}>{t("restoreImpactReplaces")}: <code>{path}</code></p>)}
    {plan.mode === "replace" && <><p>{t("restoreImpactWorkloads")}: {plan.value.containers.join(", ") || t("restoreImpactNone")}</p><ImpactList label={t("restoreImpactConflicts")} items={plan.value.conflicts.map((item) => replacementConflict(item, t))} empty={t("restoreImpactNone")} /><p>{t("restorePlanRollback")}</p></>}
    <p className="restore-panel__phrase">{phrase}</p>
    <label><span>{t("restorePlanConfirmLabel")}</span><input value={model.confirmation} placeholder={t("restoreConfirmPlaceholder")} onChange={(e) => model.setConfirmation(e.target.value)} /></label>
    <div className="signing-confirm__actions"><button className="button button--secondary" disabled={model.busy} onClick={() => model.setPlan(undefined)}>{t("restoreCancel")}</button>
      <button className="button button--primary" disabled={model.busy || model.confirmation !== phrase || (plan.mode === "replace" && plan.value.conflicts.length > 0)} onClick={() => void model.execute()}>{model.busy ? <LoaderCircle className="spin" size={16} /> : <RotateCcw size={16} />}{model.busy ? t("restoreExecuting") : t("restoreExecute")}</button>
      {model.runId && <button className="button button--secondary" disabled={model.cancelling} onClick={() => void model.cancel()}>{model.cancelling && <LoaderCircle className="spin" size={15} />}{t("restoreCancelRunning")}</button>}
    </div>
  </div>;
}

function ImpactList({ label, items, empty }: { label: string; items: string[]; empty: string }) {
  return <div><strong>{label}</strong>{items.length === 0 ? <p>{empty}</p> : <ul>{items.map((item) => <li key={item}><code>{item}</code></li>)}</ul>}</div>;
}

function replacementConflict(value: string, t: Translate): string {
  const detail = value.split(":", 2)[1];
  return detail ? `${t("restoreFailureChanged")}: ${detail}` : t("restoreFailureChanged");
}
