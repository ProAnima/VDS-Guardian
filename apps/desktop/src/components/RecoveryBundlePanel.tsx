import { useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, KeyRound, LoaderCircle } from "lucide-react";
import {
  exportRecoveryBundle, hasTauriRuntime, listRepositories, pickRecoveryBundlePath,
  type RepositorySummary,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import type { Translate } from "../i18n";

export function RecoveryBundlePanel({ resourcesRevision, t }: { resourcesRevision: number; t: Translate }) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [repositoryId, setRepositoryId] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [passphraseConfirmation, setPassphraseConfirmation] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  useEffect(() => { void listRepositories().then((items) => { const ready = items.filter((item) => item.recoveryReady); setRepositories(ready); setRepositoryId(ready[0]?.repositoryId ?? ""); }).catch((reason: unknown) => setError(errorText(reason, t))); }, [resourcesRevision, t]);
  const expected = `EXPORT RECOVERY BUNDLE FOR ${repositoryId}`;
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const outputPath = await pickRecoveryBundlePath();
    if (!outputPath) return;
    setWorking(true); setMessage(undefined); setError(undefined);
    try {
      await exportRecoveryBundle({ repositoryId, passphrase, passphraseConfirmation, outputPath, confirmation });
      setPassphrase(""); setPassphraseConfirmation(""); setConfirmation(""); setMessage(t("recoveryExportSuccess"));
    } catch (reason) { setError(errorText(reason, t)); } finally { setWorking(false); }
  };
  return <section className="repository-panel" aria-labelledby="recovery-bundle-title">
    <header className="repository-panel__header"><div><p className="eyebrow"><KeyRound size={15} />{t("recoveryExportEyebrow")}</p><h2 id="recovery-bundle-title">{t("recoveryExportTitle")}</h2><p>{t("recoveryExportBody")}</p></div></header>
    <form className="repository-form" onSubmit={(event) => void submit(event)}>
      <label><span>{t("setupStorage")}</span><select value={repositoryId} onChange={(event) => setRepositoryId(event.target.value)} required>{repositories.map((item) => <option key={item.repositoryId} value={item.repositoryId}>{item.label}</option>)}</select></label>
      <label><span>{t("recoveryPassphrase")}</span><input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} autoComplete="new-password" required /></label>
      <label><span>{t("recoveryPassphraseRepeat")}</span><input type="password" value={passphraseConfirmation} onChange={(event) => setPassphraseConfirmation(event.target.value)} autoComplete="new-password" required /></label>
      <label><span>{t("recoveryConfirmation")}</span><input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} placeholder={expected} required /></label>
      <div className="repository-form__actions"><button className="button button--primary" disabled={working || !repositoryId || passphrase !== passphraseConfirmation || !hasTauriRuntime()} type="submit">{working ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{working ? t("recoveryExporting") : t("recoveryExportAction")}</button></div>
    </form>
    {message && <p className="repository-panel__success" role="status"><CircleCheck size={16} />{message}</p>}
    {error && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{error}</p>}
    {!hasTauriRuntime() && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{t("recoveryDesktopOnly")}</p>}
  </section>;
}

function errorText(error: unknown, t: Translate): string {
  return safeErrorText(error, t("recoveryExportError"));
}
