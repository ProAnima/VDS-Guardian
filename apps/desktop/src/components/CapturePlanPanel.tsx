import { useEffect, useState, type FormEvent } from "react";
import { Check, Database, LoaderCircle } from "lucide-react";
import type { Translate } from "../i18n";
import { captureErrorText } from "../shared/capture-error";
import { togglePathSelection } from "../shared/backup-selection";
import {
  cancelJob, hasTauriRuntime, listRepositories, listSshProfiles, previewCaptureSelection,
  runCaptureSelection, type BackupSelectionItem, type CaptureSelectionPreview,
  type RepositorySummary, type SshProfileSummary,
} from "../shared/commands";
import { newRunId } from "../shared/run-id";
import { CaptureSelectionReview } from "./CaptureSelectionReview";
import { BackupSelectionSummary } from "./BackupSelectionSummary";
import { OperationFailureNotice } from "./OperationFailureNotice";
import { CaptureSourcePicker } from "./CaptureSourcePicker";

interface CapturePlanPanelProps { onPlansChanged: () => void; resourcesRevision: number; t: Translate; }

export function CapturePlanPanel({ onPlansChanged, resourcesRevision, t }: CapturePlanPanelProps) {
  const model = useCaptureSelection(onPlansChanged, resourcesRevision, t);
  return <section className="repository-panel" aria-labelledby="plan-title">
    <header className="repository-panel__header"><h2 id="plan-title">{t("backupChooseDataTitle")}</h2></header>
    <SelectionForm model={model} t={t} />
    {model.preview && <CaptureSelectionReview preview={model.preview} saving={model.working} onSave={() => void model.run()} t={t} />}
    {model.running && <RunPlanControls model={model} t={t} />}
    {model.result && <p className="repository-panel__success"><Check size={16} />{model.result}</p>}
    {model.failure && <OperationFailureNotice message={model.failure} safe="captureFailureSafe" changed="captureFailureChanged" t={t} />}
  </section>;
}

function SelectionForm({ model, t }: { model: CaptureSelectionModel; t: Translate }) {
  return <form className="repository-form" onSubmit={(event) => void model.review(event)}>
    <label><span>{t("setupServer")}</span><select value={model.profileId} onChange={(event) => model.changeProfile(event.target.value)} required>{model.profiles.map((profile) => <option key={profile.profileId} value={profile.profileId}>{profile.label}</option>)}</select></label>
    <label><span>{t("setupStorage")}</span><select value={model.repositoryId} onChange={(event) => model.changeRepository(event.target.value)} required>{model.repositories.map((repository) => <option key={repository.repositoryId} value={repository.repositoryId}>{repository.label}</option>)}</select></label>
    <div className="capture-workspace repository-form__actions">
      <CaptureSourcePicker profileId={model.profileId} items={model.items} selectedPaths={itemPaths(model.items)} onTogglePath={model.toggleRemotePath} onToggleDocker={model.toggleDockerItem} t={t} />
      <BackupSelectionSummary items={model.items} onClear={model.clearItems} onRemove={model.removeItem} t={t} />
    </div>
    <label className="capture-sqlite repository-form__actions"><span className="capture-sqlite__icon"><Database size={16} /></span><span><strong>{t("captureDatabasePath")}</strong><small>{t("captureDatabaseHint")}</small></span><input value={model.databasePath} onChange={(event) => model.changeDatabase(event.target.value)} placeholder="/srv/app/app.sqlite" /></label>
    <div className="capture-primary-action repository-form__actions"><div><strong>{t("backupReadyTitle")}</strong><span>{model.items.length === 0 ? t("backupReadyEmpty") : t("backupReadyBody")}</span></div><button className="button button--primary" disabled={!model.profileId || !model.repositoryId || model.items.length === 0 || model.reviewing || model.running} type="submit">{model.reviewing ? <LoaderCircle className="spin" size={16} /> : <Check size={16} />}{model.reviewing ? t("captureReviewing") : t("backupReview")}</button></div>
  </form>;
}

function RunPlanControls({ model, t }: { model: CaptureSelectionModel; t: Translate }) {
  return <div className="repository-form__actions"><span>{t("captureRunning")}</span><button className="button button--secondary" type="button" onClick={() => void model.cancel()}>{t("captureCancel")}</button></div>;
}

function useCaptureSelection(onPlansChanged: () => void, resourcesRevision: number, t: Translate) {
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]); const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [profileId, setProfileId] = useState(""); const [repositoryId, setRepositoryId] = useState(""); const [items, setItems] = useState<BackupSelectionItem[]>([]); const [databasePath, setDatabasePath] = useState("");
  const [preview, setPreview] = useState<CaptureSelectionPreview>(); const [reviewing, setReviewing] = useState(false); const [working, setWorking] = useState(false); const [running, setRunning] = useState(false); const [runId, setRunId] = useState<string>(); const [result, setResult] = useState<string>(); const [failure, setFailure] = useState<string>();
  const invalidate = () => { setPreview(undefined); setResult(undefined); };
  const changeProfile = (value: string) => { setProfileId(value); setItems([]); invalidate(); };
  const changeRepository = (value: string) => { setRepositoryId(value); invalidate(); };
  const changeDatabase = (value: string) => { setDatabasePath(value); invalidate(); };
  const toggleRemotePath = (path: string) => {
    setItems((current) => togglePathSelection(current, path)); invalidate();
  };
  const toggleDockerItem = (item: BackupSelectionItem) => { setItems((current) => current.some((candidate) => selectionKey(candidate) === selectionKey(item)) ? current.filter((candidate) => selectionKey(candidate) !== selectionKey(item)) : [...current, item]); invalidate(); };
  const removeItem = (item: BackupSelectionItem) => { setItems((current) => current.filter((candidate) => selectionKey(candidate) !== selectionKey(item))); invalidate(); };
  const clearItems = () => { setItems([]); invalidate(); };
  useEffect(() => { void loadResources(setProfiles, setRepositories, setProfileId, setRepositoryId); }, [resourcesRevision]);
  const review = async (event: FormEvent) => { event.preventDefault(); if (!hasTauriRuntime()) return; setReviewing(true); setFailure(undefined); try { setPreview(await previewCaptureSelection({ profileId, repositoryId, items, sqlitePath: databasePath.trim() || undefined })); } catch { setFailure(t("captureReviewFailed")); } finally { setReviewing(false); } };
  const run = async () => { if (!preview) return; const nextRunId = newRunId(); setRunId(nextRunId); setWorking(true); setRunning(true); setFailure(undefined); try { const job = await runCaptureSelection({ selection: { profileId, repositoryId, items, sqlitePath: databasePath.trim() || undefined }, confirmation: preview.confirmation, runId: nextRunId }); onPlansChanged(); setResult(`${t("captureSealed")} ${job.backupId}`); } catch (error) { setFailure(captureErrorText(error, t("captureErrorFallback"))); } finally { setWorking(false); setRunning(false); setRunId(undefined); } };
  const cancel = async () => { if (runId) await cancelJob(runId); };
  return { profiles, repositories, profileId, repositoryId, items, databasePath, preview, reviewing, working, running, result, failure, changeProfile, changeRepository, changeDatabase, toggleRemotePath, toggleDockerItem, removeItem, clearItems, review, run, cancel };
}

type CaptureSelectionModel = ReturnType<typeof useCaptureSelection>;

async function loadResources(setProfiles: (items: SshProfileSummary[]) => void, setRepositories: (items: RepositorySummary[]) => void, setProfileId: (id: string) => void, setRepositoryId: (id: string) => void) {
  const [profiles, repositories] = await Promise.all([listSshProfiles(), listRepositories()]); const ready = repositories.filter((item) => item.recoveryReady);
  setProfiles(profiles); setRepositories(ready); setProfileId(profiles[0]?.profileId ?? ""); setRepositoryId(ready[0]?.repositoryId ?? "");
}

function pathsForItem(item: BackupSelectionItem): string[] { if (item.kind === "remote_path") return [item.absolutePath]; if (item.kind === "docker_mount") return [item.capturablePath]; return item.capturablePaths; }
function itemPaths(items: BackupSelectionItem[]): string[] { return items.flatMap(pathsForItem); }
function selectionKey(item: BackupSelectionItem): string { return JSON.stringify(item); }
