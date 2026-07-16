import { useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, KeyRound, LoaderCircle } from "lucide-react";
import {
  exportRecoveryBundle, hasTauriRuntime, listRepositories, pickRecoveryBundlePath,
  type RepositoryFailure, type RepositorySummary,
} from "../shared/commands";

export function RecoveryBundlePanel() {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [repositoryId, setRepositoryId] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState<string>();
  useEffect(() => { void listRepositories().then((items) => { setRepositories(items); setRepositoryId(items[0]?.repositoryId ?? ""); }); }, []);
  const expected = `EXPORT RECOVERY BUNDLE FOR ${repositoryId}`;
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const outputPath = await pickRecoveryBundlePath();
    if (!outputPath) return;
    setWorking(true); setMessage(undefined);
    try {
      await exportRecoveryBundle({ repositoryId, passphrase, outputPath, confirmation });
      setPassphrase(""); setConfirmation(""); setMessage("Recovery bundle экспортирован. Сохраните его отдельно от диска с бэкапами.");
    } catch (error) { setMessage(errorText(error)); } finally { setWorking(false); }
  };
  return <section className="repository-panel" aria-labelledby="recovery-bundle-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><KeyRound size={15} />Recovery bundle</p><h2 id="recovery-bundle-title">Экспортировать ключ восстановления</h2><p>Файл защищён паролем. Пароль хранится только в памяти на время экспорта.</p></div></header>
    <form className="repository-form" onSubmit={(event) => void submit(event)}>
      <label><span>Хранилище</span><select value={repositoryId} onChange={(event) => setRepositoryId(event.target.value)} required>{repositories.map((item) => <option key={item.repositoryId} value={item.repositoryId}>{item.label}</option>)}</select></label>
      <label><span>Пароль для bundle</span><input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} autoComplete="new-password" required /></label>
      <label><span>Подтверждение</span><input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} placeholder={expected} required /></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={working || !repositoryId || !hasTauriRuntime()} type="submit">{working ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{working ? "Экспортируем…" : "Сохранить recovery bundle"}</button></div>
    </form>
    {message && <p className="repository-panel__success" role="status"><CircleCheck size={16} />{message}</p>}
    {!hasTauriRuntime() && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />Доступно только в desktop-приложении.</p>}
  </section>;
}

function errorText(error: unknown): string {
  if (isRepositoryFailure(error)) return `${error.message} ${error.remediation}`;
  return "Не удалось экспортировать recovery bundle.";
}

function isRepositoryFailure(error: unknown): error is RepositoryFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error;
}
