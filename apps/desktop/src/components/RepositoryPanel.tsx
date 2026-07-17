import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, FolderArchive, HardDrive, LoaderCircle } from "lucide-react";
import {
  hasTauriRuntime, initializeRepositoryRecovery, listRepositories, pickRepositoryPath, registerRepository,
  type RepositoryFailure, type RepositoryRequest, type RepositorySummary,
} from "../shared/commands";

const emptyForm: RepositoryRequest = { label: "", path: "" };

export function RepositoryPanel({ onRepositoriesChanged }: { onRepositoriesChanged: () => void }) {
  const model = useRepository(onRepositoriesChanged);
  return <section className="repository-panel" aria-labelledby="repository-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><HardDrive size={15} aria-hidden="true" />Хранилище</p><h2 id="repository-title">Выбрать место для бэкапов</h2><p>Одна выделенная папка. В ней приложение создаст независимое хранилище recovery points.</p></div><span className="signing-state"><FolderArchive size={16} />Локально</span></header>
    <form className="repository-form" onSubmit={(event) => void model.submit(event)}>
      <label><span>Название</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Recovery disk" required maxLength={128} /></label>
      <label><span>Папка для бэкапов</span><span className="path-picker"><input value={model.form.path} onChange={(event) => model.setForm({ ...model.form, path: event.target.value })} placeholder="Выберите папку" required /><button type="button" onClick={() => void pickRepositoryPath().then((path) => path && model.setForm({ ...model.form, path }))}>Обзор…</button></span></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <FolderArchive size={16} />}{model.working ? "Создаём хранилище…" : "Создать хранилище"}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">Доступно в desktop-приложении</span>}</div>
    </form>
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="repository-panel__success"><CircleCheck size={16} />{model.result}</p>}
    {model.repositories.length > 0 && <div className="repository-panel__items">{model.repositories.map((repository) => <span key={repository.repositoryId}>{repository.label} · {repository.path} · {repository.recoveryReady ? "recovery готово" : "recovery не настроено"}{!repository.recoveryReady && <button className="button button--secondary" type="button" disabled={model.working} onClick={() => void model.prepare(repository)}>Подготовить recovery</button>}</span>)}</div>}
  </section>;
}

function useRepository(onRepositoriesChanged: () => void) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [working, setWorking] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const refresh = useCallback(async () => {
    try { setRepositories(await listRepositories()); } catch (error) { setFailure(errorText(error)); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!hasTauriRuntime()) return;
    setWorking(true); setFailure(undefined); setResult(undefined);
    try {
      const repository = await registerRepository(form);
      await initializeRepositoryRecovery(repository.repositoryId);
      await refresh(); setForm(emptyForm); onRepositoriesChanged();
      setResult(`Хранилище «${repository.label}» создано и защищено recovery-ключом.`);
    } catch (error) { await refresh(); onRepositoriesChanged(); setFailure(errorText(error)); } finally { setWorking(false); }
  };
  const prepare = async (repository: RepositorySummary) => {
    setWorking(true); setFailure(undefined); setResult(undefined);
    try {
      await initializeRepositoryRecovery(repository.repositoryId);
      await refresh(); onRepositoriesChanged();
      setResult(`Recovery для «${repository.label}» готово.`);
    } catch (error) { await refresh(); onRepositoriesChanged(); setFailure(errorText(error)); } finally { setWorking(false); }
  };
  return { repositories, form, working, result, failure, setForm, submit, prepare };
}

function errorText(error: unknown): string {
  if (isRepositoryFailure(error)) return `${error.message} ${error.remediation}`;
  return "Не удалось безопасно создать локальное хранилище.";
}

function isRepositoryFailure(error: unknown): error is RepositoryFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error && typeof error.message === "string" && typeof error.remediation === "string";
}
