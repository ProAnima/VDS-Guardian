import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, FolderArchive, HardDrive, LoaderCircle } from "lucide-react";
import {
  hasTauriRuntime, initializeRepositoryRecovery, listRepositories, pickRepositoryPath, registerRepository,
  type RepositoryRequest, type RepositorySummary,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import type { Translate } from "../i18n";

const emptyForm: RepositoryRequest = { label: "", path: "" };

export function RepositoryPanel({ onRepositoriesChanged, t }: { onRepositoriesChanged: () => void; t: Translate }) {
  const model = useRepository(onRepositoriesChanged, t);
  return <section className="repository-panel" aria-labelledby="repository-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><HardDrive size={15} aria-hidden="true" />{t("setupRepositoryEyebrow")}</p><h2 id="repository-title">{t("setupRepositoryTitle")}</h2><p>{t("setupRepositoryBody")}</p></div><span className="signing-state"><FolderArchive size={16} />{t("setupLocal")}</span></header>
    <form className="repository-form" onSubmit={(event) => void model.submit(event)}>
      <label><span>{t("setupLabel")}</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Recovery disk" required maxLength={128} /></label>
      <label><span>{t("setupFolder")}</span><span className="path-picker"><input value={model.form.path} onChange={(event) => model.setForm({ ...model.form, path: event.target.value })} placeholder={t("setupPathPlaceholder")} required /><button type="button" onClick={() => void pickRepositoryPath().then((path) => path && model.setForm({ ...model.form, path }))}>{t("setupBrowse")}</button></span></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <FolderArchive size={16} />}{model.working ? t("setupCreatingRepository") : t("setupCreateRepository")}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">{t("setupDesktopOnly")}</span>}</div>
    </form>
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="repository-panel__success"><CircleCheck size={16} />{model.result}</p>}
    {model.repositories.length > 0 && <div className="repository-panel__items">{model.repositories.map((repository) => <span key={repository.repositoryId}>{repository.label} · {repository.path} · {repository.recoveryReady ? t("setupRepositoryReady") : t("setupRepositoryNotReady")}{!repository.recoveryReady && <button className="button button--secondary" type="button" disabled={model.working} onClick={() => void model.prepare(repository)}>{t("setupPrepareRecovery")}</button>}</span>)}</div>}
  </section>;
}

function useRepository(onRepositoriesChanged: () => void, t: Translate) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [working, setWorking] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const refresh = useCallback(async () => {
    try { setRepositories(await listRepositories()); } catch (error) { setFailure(errorText(error, t)); }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!hasTauriRuntime()) return;
    setWorking(true); setFailure(undefined); setResult(undefined);
    try {
      const repository = await registerRepository(form);
      await initializeRepositoryRecovery(repository.repositoryId);
      await refresh(); setForm(emptyForm); onRepositoriesChanged();
      setResult(`${t("setupRepositoryCreated")} ${repository.label}`);
    } catch (error) { await refresh(); onRepositoriesChanged(); setFailure(errorText(error, t)); } finally { setWorking(false); }
  };
  const prepare = async (repository: RepositorySummary) => {
    setWorking(true); setFailure(undefined); setResult(undefined);
    try {
      await initializeRepositoryRecovery(repository.repositoryId);
      await refresh(); onRepositoriesChanged();
      setResult(`${t("setupRecoveryPrepared")} ${repository.label}`);
    } catch (error) { await refresh(); onRepositoriesChanged(); setFailure(errorText(error, t)); } finally { setWorking(false); }
  };
  return { repositories, form, working, result, failure, setForm, submit, prepare };
}

function errorText(error: unknown, t: Translate): string {
  return safeErrorText(error, t("setupRepositoryError"));
}
