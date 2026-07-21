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

interface RestorePanelProps { t: Translate; }
type RestoreMode = "separate" | "replace";
type Plan = { mode: "separate"; value: DeploymentPreview } | { mode: "replace"; value: ReplacementResult };

export function RestorePanel({ t }: RestorePanelProps) {
  const model = useRestoreModel(t);
  return <main className="dashboard">
    <section className="hero-panel"><div className="hero-panel__content">
      <p className="eyebrow"><RotateCcw size={15} />{t("restoreEyebrow")}</p>
      <h1>{t("restoreTitle")}</h1><p>{t("restoreBody")}</p>
    </div></section>
    <section className="repository-panel" aria-labelledby="restore-title">
      <header className="repository-panel__header"><h2 id="restore-title">{t("restoreBackupsTitle")}</h2></header>
      {model.plan ? <Confirmation model={model} t={t} /> : <RestoreForm model={model} t={t} />}
      {model.result && <p className="repository-panel__success"><Check size={16} />{model.result}</p>}
      {model.failure && <OperationFailureNotice message={model.failure} safe="restoreFailureSafe" changed="restoreFailureChanged" t={t} />}
      {!hasTauriRuntime() && <p className="signing-panel__desktop">{t("restoreDesktopRequired")}</p>}
    </section>
  </main>;
}

function useRestoreModel(t: Translate) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [backups, setBackups] = useState<BackupSummary[]>([]);
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [repositoryId, setRepositoryId] = useState("");
  const [backupId, setBackupId] = useState("");
  const [profileId, setProfileId] = useState("");
  const [description, setDescription] = useState<BackupRestoreDescription>();
  const [mode, setMode] = useState<RestoreMode>("separate");
  const [targetPath, setTargetPath] = useState("");
  const action = useRestoreAction(t, { repositoryId, backupId, profileId, mode, targetPath });
  const setFailure = action.setFailure;
  useEffect(() => { void Promise.all([listRepositories(), listSshProfiles()]).then(([repos, servers]) => {
    setRepositories(repos); setProfiles(servers); setRepositoryId(repos[0]?.repositoryId ?? "");
  }).catch((error: unknown) => setFailure(safeErrorText(error, t("restoreErrorFallback")))); }, [setFailure, t]);
  useEffect(() => { if (!repositoryId) return; void listBackups(repositoryId).then((items) => {
    setBackups(items); setBackupId(items[0]?.backupId ?? "");
  }).catch((error: unknown) => setFailure(safeErrorText(error, t("restoreErrorFallback")))); }, [repositoryId, setFailure, t]);
  useEffect(() => { setDescription(undefined); if (!repositoryId || !backupId) return;
    void inspectRestoreBackup(repositoryId, backupId).then((value) => {
      setDescription(value); setProfileId(value.sourceProfileId); setTargetPath(value.roots[0] ?? "");
      if (!value.replacementAvailable) setMode("separate");
    }).catch((error: unknown) => setFailure(safeErrorText(error, t("restoreErrorFallback"))));
  }, [repositoryId, backupId, setFailure, t]);
  return { repositories, backups, profiles, repositoryId, setRepositoryId, backupId, setBackupId,
    profileId, setProfileId, description, mode, setMode, targetPath, setTargetPath, ...action };
}

interface ActionInput { repositoryId: string; backupId: string; profileId: string; mode: RestoreMode; targetPath: string; }
function useRestoreAction(t: Translate, input: ActionInput) {
  const [plan, setPlan] = useState<Plan>(); const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false); const [runId, setRunId] = useState<string>();
  const [result, setResult] = useState<string>(); const [failure, setFailure] = useState<string>();
  const preview = (event: FormEvent) => { event.preventDefault(); void runPreview(); };
  const runPreview = async () => { setBusy(true); setFailure(undefined); try {
    if (input.mode === "replace") setPlan({ mode: "replace", value: await previewSourceReplacement(request(input)) });
    else setPlan({ mode: "separate", value: await previewDeploy({ ...request(input), targetPath: input.targetPath }) });
    setConfirmation("");
  } catch (error) { setFailure(safeErrorText(error, t("restoreErrorFallback"))); } finally { setBusy(false); } };
  const execute = async () => { if (!plan) return; const id = newRunId(); setRunId(id); setBusy(true); setFailure(undefined); try {
    if (plan.mode === "replace") { const done = await executeSourceReplacement({ ...request(input), confirmation, runId: id }); setResult(`${t("restoreSuccess")} ${done.root}`); }
    else { const done = await executeDeploy({ ...request(input), targetPath: input.targetPath, confirmation, runId: id }); setResult(`${t("restoreSuccess")} ${done.targetPath}`); }
    setPlan(undefined); setConfirmation("");
  } catch (error) { setFailure(safeErrorText(error, t("restoreErrorFallback"))); } finally { setBusy(false); setRunId(undefined); } };
  return { plan, setPlan, confirmation, setConfirmation, busy, runId, result, failure, setFailure, preview, execute };
}

function request(input: ActionInput) { return { repositoryId: input.repositoryId, backupId: input.backupId, targetProfileId: input.profileId }; }
type Model = ReturnType<typeof useRestoreModel>;

function RestoreForm({ model, t }: { model: Model; t: Translate }) {
  if (model.repositories.length === 0) return <p className="restore-panel__empty">{t("restoreNoRepositories")}</p>;
  return <form className="repository-form" onSubmit={model.preview}>
    <label><span>{t("restoreRepository")}</span><select value={model.repositoryId} onChange={(e) => model.setRepositoryId(e.target.value)}>{model.repositories.map((item) => <option key={item.repositoryId} value={item.repositoryId}>{item.label}</option>)}</select></label>
    <label><span>{t("restoreBackupsTitle")}</span><select value={model.backupId} onChange={(e) => model.setBackupId(e.target.value)}>{model.backups.map((item) => <option key={item.backupId} value={item.backupId}>{item.backupId} — {item.sealedAt}</option>)}</select></label>
    {model.description && <BackupExplorer description={model.description} t={t} />}
    <div className="restore-mode" role="radiogroup">
      <button className={`button ${model.mode === "separate" ? "button--primary" : "button--secondary"}`} type="button" onClick={() => model.setMode("separate")}>{t("restoreDestination")}</button>
      <button className={`button ${model.mode === "replace" ? "button--primary" : "button--secondary"}`} type="button" disabled={!model.description?.replacementAvailable} onClick={() => model.setMode("replace")}>{t("restoreImpactReplaces")}</button>
    </div>
    <label><span>{t("deployTargetProfile")}</span><select value={model.profileId} disabled={model.mode === "replace"} onChange={(e) => model.setProfileId(e.target.value)}>{model.profiles.map((item) => <option key={item.profileId} value={item.profileId}>{item.label}</option>)}</select></label>
    <label><span>{t("deployTargetPath")}</span><input value={model.targetPath} readOnly={model.mode === "replace"} onChange={(e) => model.setTargetPath(e.target.value)} placeholder={t("deployTargetPathHint")} /></label>
    <button className="button button--primary" disabled={model.busy || !model.backupId || !model.profileId || !model.targetPath} type="submit">{model.busy ? <LoaderCircle className="spin" size={16} /> : <Eye size={16} />}{model.busy ? t("restorePreviewing") : t("restorePreview")}</button>
  </form>;
}

function BackupExplorer({ description, t }: { description: BackupRestoreDescription; t: Translate }) {
  return <div className="restore-explorer">
    <div><strong><Server size={15} /> {t("restorePlanSource")}</strong>{description.roots.map((root) => <code key={root}>{root}</code>)}</div>
    <div><strong><File size={15} /> {t("restoreImpactAdds")} ({description.entries.length}/{description.totalEntries})</strong><ul>{description.entries.map((entry) => <li key={entry.path}>{entry.kind === "directory" ? <Folder size={14} /> : <File size={14} />}<code>{entry.path}</code></li>)}</ul></div>
    {description.dockerWorkloads.length > 0 && <div><strong><Box size={15} /> {t("restoreImpactWorkloads")}</strong><ul>{description.dockerWorkloads.map((item) => <li key={item.containerId}><code>{item.containerName}</code> · {item.image} · {t(activeState(item.state) ? "dockerActive" : "dockerStopped")}</li>)}</ul></div>}
  </div>;
}

function activeState(state: string): boolean { return ["running", "paused", "restarting"].includes(state); }

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
      {model.runId && <button className="button button--secondary" onClick={() => void cancelJob(model.runId ?? "")}>{t("restoreCancelRunning")}</button>}
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
