import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, FolderArchive, HardDrive, LoaderCircle } from "lucide-react";
import {
  hasTauriRuntime, listRepositories, pickRepositoryPath, registerRepository,
  type RepositoryFailure, type RepositoryRequest, type RepositorySummary,
} from "../shared/commands";

const emptyForm: RepositoryRequest = { label: "", path: "" };

export function RepositoryPanel() {
  const model = useRepository();
  return <section className="repository-panel" aria-labelledby="repository-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><HardDrive size={15} aria-hidden="true" />Хранилище</p><h2 id="repository-title">Выбрать место для бэкапов</h2><p>Одна выделенная папка. В ней приложение создаст независимое хранилище recovery points.</p></div><span className="signing-state"><FolderArchive size={16} />Локально</span></header>
    <form className="repository-form" onSubmit={(event) => void model.submit(event)}>
      <label><span>Название</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Recovery disk" required maxLength={128} /></label>
      <label><span>Папка для бэкапов</span><span className="path-picker"><input value={model.form.path} onChange={(event) => model.setForm({ ...model.form, path: event.target.value })} placeholder="Выберите папку" required /><button type="button" onClick={() => void pickRepositoryPath().then((path) => path && model.setForm({ ...model.form, path }))}>Обзор…</button></span></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <FolderArchive size={16} />}{model.working ? "Создаём хранилище…" : "Создать хранилище"}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">Доступно в desktop-приложении</span>}</div>
    </form>
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="repository-panel__success"><CircleCheck size={16} />{model.result}</p>}
    {model.repositories.length > 0 && <div className="repository-panel__items">{model.repositories.map((repository) => <span key={repository.repositoryId}>{repository.label} · {repository.path}</span>)}</div>}
  </section>;
}

function useRepository() {
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
      setRepositories((current) => [...current, repository]); setForm(emptyForm);
      setResult(`Хранилище «${repository.label}» создано и готово к настройке capture plan.`);
    } catch (error) { setFailure(errorText(error)); } finally { setWorking(false); }
  };
  return { repositories, form, working, result, failure, setForm, submit };
}

function errorText(error: unknown): string {
  if (isRepositoryFailure(error)) return `${error.message} ${error.remediation}`;
  return "Не удалось безопасно создать локальное хранилище.";
}

function isRepositoryFailure(error: unknown): error is RepositoryFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error && typeof error.message === "string" && typeof error.remediation === "string";
}
