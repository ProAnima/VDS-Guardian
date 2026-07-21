import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, FolderArchive, HardDrive, LoaderCircle, Pencil, Trash2 } from "lucide-react";
import {
  deleteRepository, hasTauriRuntime, initializeRepositoryRecovery, listRepositories,
  pickRepositoryPath, registerRepository, updateRepositoryPath,
  type RepositoryRequest, type RepositorySummary,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import type { Translate } from "../i18n";

const emptyForm: RepositoryRequest = { label: "", path: "" };

export function RepositoryPanel({ onRepositoriesChanged, t }: { onRepositoriesChanged: () => void; t: Translate }) {
  const model = useRepository(onRepositoriesChanged, t);
  return <section className="repository-panel" aria-labelledby="repository-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><HardDrive size={15} aria-hidden="true" />{t("setupRepositoryEyebrow")}</p><h2 id="repository-title">{t("setupRepositoryTitle")}</h2><p>{t("setupRepositoryBody")}</p></div><span className="signing-state"><FolderArchive size={16} />{t("setupLocal")}</span></header>
    <RepositoryForm model={model} t={t} />
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="repository-panel__success"><CircleCheck size={16} />{model.result}</p>}
    <RepositoryCards model={model} t={t} />
  </section>;
}

function RepositoryForm({ model, t }: { model: RepositoryModel; t: Translate }) {
  return <form className="repository-form" onSubmit={(event) => void model.submit(event)}>
    <label><span>{t("setupLabel")}</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Recovery disk" required maxLength={128} /></label>
    <label><span>{t("setupFolder")}</span><span className="path-picker"><input value={model.form.path} onChange={(event) => model.setForm({ ...model.form, path: event.target.value })} placeholder={t("setupPathPlaceholder")} required /><button type="button" onClick={() => void model.pickNewPath()}>{t("setupBrowse")}</button></span></label>
    <div className="repository-form__actions"><button className="button button--primary" disabled={model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <FolderArchive size={16} />}{model.working ? t("setupCreatingRepository") : t("setupCreateRepository")}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">{t("setupDesktopOnly")}</span>}</div>
  </form>;
}

function RepositoryCards({ model, t }: { model: RepositoryModel; t: Translate }) {
  if (model.repositories.length === 0) return null;
  return <div className="repository-list">{model.repositories.map((repository) =>
    <RepositoryCard key={repository.repositoryId} repository={repository} model={model} t={t} />
  )}</div>;
}

function RepositoryCard({ repository, model, t }: { repository: RepositorySummary; model: RepositoryModel; t: Translate }) {
  const editing = model.editing?.repositoryId === repository.repositoryId;
  const confirming = model.confirmingId === repository.repositoryId;
  return <article className="repository-card">
    <div className="repository-card__main"><strong>{repository.label}</strong><span title={repository.path}>{repository.path}</span><small>{repository.recoveryReady ? t("setupRepositoryReady") : t("setupRepositoryNotReady")}</small></div>
    {!repository.recoveryReady && <button className="button button--secondary" type="button" disabled={model.working} onClick={() => void model.prepare(repository)}>{t("setupPrepareRecovery")}</button>}
    {editing && <RepositoryPathEditor model={model} t={t} />}
    {confirming && <RepositoryDeleteConfirmation repository={repository} model={model} t={t} />}
    {!editing && !confirming && <div className="repository-card__actions"><button type="button" onClick={() => model.startEditing(repository)}><Pencil size={14} />{t("repositoryChangeFolder")}</button><button className="repository-card__remove" type="button" onClick={() => model.setConfirmingId(repository.repositoryId)}><Trash2 size={14} />{t("repositoryDelete")}</button></div>}
  </article>;
}

function RepositoryPathEditor({ model, t }: { model: RepositoryModel; t: Translate }) {
  if (!model.editing) return null;
  return <form className="repository-card__editor" onSubmit={(event) => void model.savePath(event)}>
    <small>{t("repositoryPathHint")}</small><span className="path-picker"><input aria-label={t("setupFolder")} value={model.editing.path} onChange={(event) => model.setEditing({ ...model.editing!, path: event.target.value })} required /><button type="button" onClick={() => void model.pickEditPath()}>{t("setupBrowse")}</button></span>
    <div><button type="button" onClick={() => model.setEditing(undefined)}>{t("repositoryCancel")}</button><button className="button button--primary" type="submit" disabled={model.working}>{t("repositorySaveFolder")}</button></div>
  </form>;
}

function RepositoryDeleteConfirmation({ repository, model, t }: { repository: RepositorySummary; model: RepositoryModel; t: Translate }) {
  return <div className="repository-card__confirm" role="alert"><div><strong>{t("repositoryDeleteQuestion")}</strong><span>{t("repositoryDeleteWarning")}</span></div><div><button type="button" onClick={() => model.setConfirmingId(undefined)}>{t("repositoryCancel")}</button><button className="repository-card__delete-confirm" disabled={model.working} type="button" onClick={() => void model.remove(repository)}>{model.working ? <LoaderCircle className="spin" size={14} /> : <Trash2 size={14} />}{t("repositoryDelete")}</button></div></div>;
}

function useRepository(onRepositoriesChanged: () => void, t: Translate) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [editing, setEditing] = useState<{ repositoryId: string; path: string }>();
  const [confirmingId, setConfirmingId] = useState<string>();
  const [working, setWorking] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const refresh = useCallback(async () => {
    try { setRepositories(await listRepositories()); } catch (error) { setFailure(errorText(error, t)); }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);
  const notify = async () => { await refresh(); onRepositoriesChanged(); };
  const run = async (operation: () => Promise<string>) => {
    setWorking(true); setFailure(undefined); setResult(undefined);
    try { setResult(await operation()); } catch (error) { setFailure(errorText(error, t)); }
    finally { setWorking(false); }
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault(); if (!hasTauriRuntime()) return;
    await run(async () => { const repository = await registerRepository(form); await initializeRepositoryRecovery(repository.repositoryId); await notify(); setForm(emptyForm); return `${t("setupRepositoryCreated")} ${repository.label}`; });
  };
  const prepare = async (repository: RepositorySummary) => run(async () => { await initializeRepositoryRecovery(repository.repositoryId); await notify(); return `${t("setupRecoveryPrepared")} ${repository.label}`; });
  const savePath = async (event: FormEvent) => {
    event.preventDefault(); if (!editing) return;
    await run(async () => { const updated = await updateRepositoryPath(editing); await notify(); setEditing(undefined); return `${t("repositoryPathUpdated")} ${updated.label}`; });
  };
  const remove = async (repository: RepositorySummary) => run(async () => { await deleteRepository(repository.repositoryId); await notify(); setConfirmingId(undefined); return `${t("repositoryDeleted")} ${repository.label}`; });
  const startEditing = (repository: RepositorySummary) => { setConfirmingId(undefined); setEditing({ repositoryId: repository.repositoryId, path: repository.path }); };
  const pickNewPath = async () => { const path = await pickRepositoryPath(); if (path) setForm((current) => ({ ...current, path })); };
  const pickEditPath = async () => { const path = await pickRepositoryPath(); if (path) setEditing((current) => current && ({ ...current, path })); };
  return { repositories, form, editing, confirmingId, working, result, failure, setForm, setEditing, setConfirmingId, submit, prepare, savePath, remove, startEditing, pickNewPath, pickEditPath };
}

type RepositoryModel = ReturnType<typeof useRepository>;

function errorText(error: unknown, t: Translate): string { return safeErrorText(error, t("setupRepositoryError")); }
