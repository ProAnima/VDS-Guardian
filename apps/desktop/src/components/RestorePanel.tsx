import { useEffect, useState, type FormEvent } from "react";
import { Check, CircleAlert, Eye, LoaderCircle, RotateCcw } from "lucide-react";
import type { Translate } from "../i18n";
import {
  executeRestore, hasTauriRuntime, listBackups, listRepositories, previewRestore,
  type BackupSummary, type RepositorySummary, type RestoreFailure, type RestorePreview,
} from "../shared/commands";

interface RestorePanelProps { t: Translate; }

export function RestorePanel({ t }: RestorePanelProps) {
  const model = useRestoreModel(t);
  return (
    <main className="dashboard">
      <section className="hero-panel">
        <div className="hero-panel__content">
          <p className="eyebrow"><RotateCcw size={15} aria-hidden="true" />{t("restoreEyebrow")}</p>
          <h1>{t("restoreTitle")}</h1>
          <p>{t("restoreBody")}</p>
        </div>
      </section>
      <section className="repository-panel" aria-labelledby="restore-title">
        <header className="repository-panel__header">
          <div><h2 id="restore-title">{t("restoreBackupsTitle")}</h2></div>
        </header>
        {model.plan ? <RestoreConfirmation model={model} t={t} /> : <RestoreForm model={model} t={t} />}
        {model.result && <p className="repository-panel__success"><Check size={16} />{model.result}</p>}
        {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
        {!hasTauriRuntime() && <p className="signing-panel__desktop">{t("restoreDesktopRequired")}</p>}
      </section>
    </main>
  );
}

function useBackupSelection() {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [repositoryId, setRepositoryId] = useState("");
  const [backups, setBackups] = useState<BackupSummary[]>([]);
  const [backupId, setBackupId] = useState("");

  useEffect(() => {
    void listRepositories().then((next) => {
      setRepositories(next);
      setRepositoryId((current) => current || (next[0]?.repositoryId ?? ""));
    });
  }, []);

  useEffect(() => {
    if (!repositoryId) {
      setBackups([]);
      setBackupId("");
      return;
    }
    void listBackups(repositoryId).then((next) => {
      setBackups(next);
      setBackupId(next[0]?.backupId ?? "");
    });
  }, [repositoryId]);

  return { repositories, repositoryId, setRepositoryId, backups, backupId, setBackupId };
}

interface RestoreActionState {
  repositoryId: string;
  backupId: string;
  destination: string;
  plan?: RestorePreview;
  confirmationInput: string;
}

interface RestoreActionSetters {
  setPlan: (value: RestorePreview | undefined) => void;
  setConfirmationInput: (value: string) => void;
  setResult: (value: string | undefined) => void;
  setFailure: (value: string | undefined) => void;
}

async function submitPreview(
  event: FormEvent,
  state: RestoreActionState,
  setters: RestoreActionSetters,
  setPreviewing: (value: boolean) => void,
  t: Translate,
): Promise<void> {
  event.preventDefault();
  if (!hasTauriRuntime() || !state.repositoryId || !state.backupId || !state.destination) return;
  setPreviewing(true);
  setters.setFailure(undefined);
  try {
    setters.setPlan(await previewRestore(state));
    setters.setConfirmationInput("");
  } catch (error) {
    setters.setFailure(errorText(error, t));
  } finally {
    setPreviewing(false);
  }
}

async function submitRestore(
  state: RestoreActionState,
  setters: RestoreActionSetters,
  setRestoring: (value: boolean) => void,
  t: Translate,
): Promise<void> {
  if (!state.plan) return;
  setRestoring(true);
  setters.setFailure(undefined);
  try {
    const restored = await executeRestore({
      repositoryId: state.repositoryId,
      backupId: state.backupId,
      destination: state.destination,
      confirmation: state.confirmationInput,
    });
    setters.setResult(`${t("restoreSuccess")} ${restored.destination}`);
    setters.setPlan(undefined);
    setters.setConfirmationInput("");
  } catch (error) {
    setters.setFailure(errorText(error, t));
  } finally {
    setRestoring(false);
  }
}

function useRestoreActions(t: Translate, repositoryId: string, backupId: string, destination: string) {
  const [plan, setPlan] = useState<RestorePreview>();
  const [confirmationInput, setConfirmationInput] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const setters = { setPlan, setConfirmationInput, setResult, setFailure };
  const state = { repositoryId, backupId, destination, plan, confirmationInput };

  const preview = (event: FormEvent) => void submitPreview(event, state, setters, setPreviewing, t);
  const restore = () => void submitRestore(state, setters, setRestoring, t);
  const cancelPlan = () => { setPlan(undefined); setConfirmationInput(""); };

  return { plan, confirmationInput, setConfirmationInput, previewing, restoring, result, failure, preview, restore, cancelPlan };
}

function useRestoreModel(t: Translate) {
  const [destination, setDestination] = useState("");
  const selection = useBackupSelection();
  const actions = useRestoreActions(t, selection.repositoryId, selection.backupId, destination);
  return { ...selection, destination, setDestination, ...actions };
}

type RestoreModel = ReturnType<typeof useRestoreModel>;

function RestoreForm({ model, t }: { model: RestoreModel; t: Translate }) {
  if (model.repositories.length === 0) {
    return <p className="restore-panel__empty">{t("restoreNoRepositories")}</p>;
  }
  return (
    <form className="repository-form" onSubmit={model.preview}>
      <RepositoryField model={model} t={t} />
      <BackupField model={model} t={t} />
      <DestinationField model={model} t={t} />
      <div className="repository-form__actions">
        <PreviewButton model={model} t={t} />
      </div>
    </form>
  );
}

function RepositoryField({ model, t }: { model: RestoreModel; t: Translate }) {
  return (
    <label>
      <span>{t("restoreRepository")}</span>
      <select value={model.repositoryId} onChange={(event) => model.setRepositoryId(event.target.value)} required>
        {model.repositories.map((repository) => (
          <option key={repository.repositoryId} value={repository.repositoryId}>{repository.label}</option>
        ))}
      </select>
    </label>
  );
}

function BackupField({ model, t }: { model: RestoreModel; t: Translate }) {
  if (model.backups.length === 0) {
    return (
      <label>
        <span>{t("restoreBackupsTitle")}</span>
        <span className="restore-panel__empty">{t("restoreNoBackups")}</span>
      </label>
    );
  }
  return (
    <label>
      <span>{t("restoreBackupsTitle")}</span>
      <select value={model.backupId} onChange={(event) => model.setBackupId(event.target.value)} required>
        {model.backups.map((backup) => (
          <option key={backup.backupId} value={backup.backupId}>{backup.backupId} — {backup.sealedAt} — verified</option>
        ))}
      </select>
    </label>
  );
}

function DestinationField({ model, t }: { model: RestoreModel; t: Translate }) {
  return (
    <label className="repository-form__actions">
      <span>{t("restoreDestination")}</span>
      <input
        value={model.destination}
        onChange={(event) => model.setDestination(event.target.value)}
        placeholder={t("restoreDestinationHint")}
        required
      />
    </label>
  );
}

function PreviewButton({ model, t }: { model: RestoreModel; t: Translate }) {
  const disabled = !model.repositoryId || !model.backupId || !model.destination || model.previewing;
  return (
    <button className="button button--primary" type="submit" disabled={disabled}>
      {model.previewing ? <LoaderCircle className="spin" size={16} /> : <Eye size={16} />}
      {model.previewing ? t("restorePreviewing") : t("restorePreview")}
    </button>
  );
}

function RestoreConfirmation({ model, t }: { model: RestoreModel; t: Translate }) {
  const plan = model.plan;
  if (!plan) return null;
  const confirmed = model.confirmationInput === plan.confirmation;
  return (
    <div className="signing-confirm" aria-live="polite">
      <div>
        <strong>{t("restorePlanTitle")}</strong>
        <p>Источник: {plan.backupId}</p>
        <p>Назначение: {plan.destination}</p>
        <p>{t("restorePlanPayload")}: {plan.payload}</p>
        <p>Восстановление создаёт только новую папку назначения и не перезаписывает существующую.</p>
        <p className="restore-panel__phrase">{plan.confirmation}</p>
      </div>
      <label>
        <span>{t("restorePlanConfirmLabel")}</span>
        <input
          value={model.confirmationInput}
          onChange={(event) => model.setConfirmationInput(event.target.value)}
          placeholder={t("restoreConfirmPlaceholder")}
        />
      </label>
      <p>{t("restorePlanConfirmHint")}</p>
      <div className="signing-confirm__actions">
        <button className="button button--secondary" disabled={model.restoring} onClick={model.cancelPlan} type="button">
          {t("restoreCancel")}
        </button>
        <button className="button button--primary" disabled={!confirmed || model.restoring} onClick={model.restore} type="button">
          {model.restoring ? <LoaderCircle className="spin" size={16} /> : <RotateCcw size={16} />}
          {model.restoring ? t("restoreExecuting") : t("restoreExecute")}
        </button>
      </div>
    </div>
  );
}

function errorText(error: unknown, t: Translate): string {
  return isRestoreFailure(error) ? `${error.message} ${error.remediation}` : t("restoreErrorFallback");
}

function isRestoreFailure(error: unknown): error is RestoreFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error
    && typeof error.message === "string" && typeof error.remediation === "string";
}
