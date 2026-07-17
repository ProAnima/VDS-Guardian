import { useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, KeyRound, LoaderCircle } from "lucide-react";
import {
  hasTauriRuntime, importRecoveryBundle, pickRecoveryBundleInput, pickRepositoryPath,
  type RepositoryFailure,
} from "../shared/commands";

export function RecoveryImportPanel({ onRepositoriesChanged }: { onRepositoriesChanged: () => void }) {
  const model = useImportModel(onRepositoriesChanged);
  return <section className="repository-panel" aria-labelledby="recovery-import-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><KeyRound size={15} />Clean-machine recovery</p><h2 id="recovery-import-title">Импортировать ключ восстановления</h2><p>Сначала выберите исходное хранилище и bundle. Новое хранилище появится в приложении только после проверки пароля.</p></div></header>
    <form className="repository-form" onSubmit={(event) => void model.submit(event)}>
      <label><span>ID хранилища</span><input value={model.repositoryId} onChange={(event) => model.setRepositoryId(event.target.value)} required /></label>
      <PathField label="Папка хранилища" value={model.repositoryPath} onChange={model.setRepositoryPath} pick={pickRepositoryPath} />
      <PathField label="Recovery bundle" value={model.inputPath} onChange={model.setInputPath} pick={pickRecoveryBundleInput} />
      <label><span>Пароль bundle</span><input type="password" value={model.passphrase} onChange={(event) => model.setPassphrase(event.target.value)} required /></label>
      <label><span>Подтверждение</span><input value={model.confirmation} onChange={(event) => model.setConfirmation(event.target.value)} placeholder={`IMPORT RECOVERY BUNDLE FOR ${model.repositoryId}`} required /></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{model.working ? "Импортируем…" : "Импортировать recovery bundle"}</button></div>
    </form>
    {model.message && <p className="repository-panel__success" role="status"><CircleCheck size={16} />{model.message}</p>}
    {model.error && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.error}</p>}
  </section>;
}

function PathField({ label, value, onChange, pick }: { label: string; value: string; onChange: (value: string) => void; pick: () => Promise<string | undefined>; }) {
  return <label><span>{label}</span><span className="path-picker"><input value={value} onChange={(event) => onChange(event.target.value)} required /><button type="button" onClick={() => void pick().then((path) => path && onChange(path))}>Обзор…</button></span></label>;
}

function useImportModel(onRepositoriesChanged: () => void) {
  const [repositoryId, setRepositoryId] = useState(""); const [repositoryPath, setRepositoryPath] = useState(""); const [inputPath, setInputPath] = useState(""); const [passphrase, setPassphrase] = useState(""); const [confirmation, setConfirmation] = useState(""); const [working, setWorking] = useState(false); const [message, setMessage] = useState<string>(); const [error, setError] = useState<string>();
  const submit = async (event: FormEvent) => { event.preventDefault(); setWorking(true); setError(undefined); setMessage(undefined); try { const result = await importRecoveryBundle({ repositoryId, repositoryPath, inputPath, passphrase, confirmation }); setPassphrase(""); setConfirmation(""); onRepositoriesChanged(); setMessage(`Хранилище «${result.label}» готово к проверке и восстановлению.`); } catch (reason) { setError(errorText(reason)); } finally { setWorking(false); } };
  return { repositoryId, setRepositoryId, repositoryPath, setRepositoryPath, inputPath, setInputPath, passphrase, setPassphrase, confirmation, setConfirmation, working, message, error, submit };
}

function errorText(error: unknown): string { return isRepositoryFailure(error) ? `${error.message} ${error.remediation}` : "Не удалось импортировать recovery bundle."; }
function isRepositoryFailure(error: unknown): error is RepositoryFailure { return typeof error === "object" && error !== null && "message" in error && "remediation" in error; }
