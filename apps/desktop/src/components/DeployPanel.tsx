import { useEffect, useState, type FormEvent } from "react";
import { Check, Eye, LoaderCircle, Rocket } from "lucide-react";
import type { Translate } from "../i18n";
import { OperationFailureNotice } from "./OperationFailureNotice";
import {
  cancelJob, executeDeploy, hasTauriRuntime, listBackups, listRepositories, listSshProfiles, previewDeploy,
  type BackupSummary, type DeployFailure, type DeploymentPreview, type RepositorySummary, type SshProfileSummary,
} from "../shared/commands";

interface DeployPanelProps { t: Translate; }

export function DeployPanel({ t }: DeployPanelProps) {
  const model = useDeployModel(t);
  return (
    <main className="dashboard">
      <section className="hero-panel">
        <div className="hero-panel__content">
          <p className="eyebrow"><Rocket size={15} aria-hidden="true" />{t("deployEyebrow")}</p>
          <h1>{t("deployTitle")}</h1>
          <p>{t("deployBody")}</p>
        </div>
      </section>
      <section className="repository-panel" aria-labelledby="deploy-title">
        <header className="repository-panel__header">
          <div><h2 id="deploy-title">{t("restoreBackupsTitle")}</h2></div>
        </header>
        {model.plan ? <DeployConfirmation model={model} t={t} /> : <DeployForm model={model} t={t} />}
        {model.result && <p className="repository-panel__success"><Check size={16} />{model.result}</p>}
        {model.failure && <OperationFailureNotice message={model.failure} safe="deployFailureSafe" changed="deployFailureChanged" t={t} />}
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

function useTargetProfiles() {
  const [targetProfiles, setTargetProfiles] = useState<SshProfileSummary[]>([]);
  const [targetProfileId, setTargetProfileId] = useState("");

  useEffect(() => {
    void listSshProfiles().then((next) => {
      setTargetProfiles(next);
      setTargetProfileId((current) => current || (next[0]?.profileId ?? ""));
    });
  }, []);

  return { targetProfiles, targetProfileId, setTargetProfileId };
}

interface DeployActionState {
  repositoryId: string;
  backupId: string;
  targetProfileId: string;
  targetPath: string;
  plan?: DeploymentPreview;
  confirmationInput: string;
}

interface DeployActionSetters {
  setPlan: (value: DeploymentPreview | undefined) => void;
  setConfirmationInput: (value: string) => void;
  setResult: (value: string | undefined) => void;
  setFailure: (value: string | undefined) => void;
}

async function submitPreview(
  event: FormEvent,
  state: DeployActionState,
  setters: DeployActionSetters,
  setPreviewing: (value: boolean) => void,
  t: Translate,
): Promise<void> {
  event.preventDefault();
  if (!hasTauriRuntime() || !state.repositoryId || !state.backupId || !state.targetProfileId || !state.targetPath) return;
  setPreviewing(true);
  setters.setFailure(undefined);
  try {
    setters.setPlan(await previewDeploy(state));
    setters.setConfirmationInput("");
  } catch (error) {
    setters.setFailure(errorText(error, t));
  } finally {
    setPreviewing(false);
  }
}

async function submitDeploy(
  state: DeployActionState,
  setters: DeployActionSetters,
  setDeploying: (value: boolean) => void,
  setRunId: (value: string | undefined) => void,
  t: Translate,
): Promise<void> {
  if (!state.plan) return;
  const runId = crypto.randomUUID();
  setRunId(runId);
  setDeploying(true);
  setters.setFailure(undefined);
  try {
    const deployed = await executeDeploy({
      repositoryId: state.repositoryId,
      backupId: state.backupId,
      targetProfileId: state.targetProfileId,
      targetPath: state.targetPath,
      confirmation: state.confirmationInput,
      runId,
    });
    setters.setResult(`${t("deploySuccess")} ${deployed.targetProfileLabel}:${deployed.targetPath}`);
    setters.setPlan(undefined);
    setters.setConfirmationInput("");
  } catch (error) {
    setters.setFailure(errorText(error, t));
  } finally {
    setDeploying(false);
    setRunId(undefined);
  }
}

function useDeployActions(t: Translate, repositoryId: string, backupId: string, targetProfileId: string, targetPath: string) {
  const [plan, setPlan] = useState<DeploymentPreview>();
  const [confirmationInput, setConfirmationInput] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [deploying, setDeploying] = useState(false);
  const [runId, setRunId] = useState<string>();
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const setters = { setPlan, setConfirmationInput, setResult, setFailure };
  const state = { repositoryId, backupId, targetProfileId, targetPath, plan, confirmationInput };

  const preview = (event: FormEvent) => void submitPreview(event, state, setters, setPreviewing, t);
  const deploy = () => void submitDeploy(state, setters, setDeploying, setRunId, t);
  const cancelPlan = () => { setPlan(undefined); setConfirmationInput(""); };
  const cancelRunningDeploy = () => { if (runId) void cancelJob(runId); };

  return { plan, confirmationInput, setConfirmationInput, previewing, deploying, result, failure, preview, deploy, cancelPlan, cancelRunningDeploy };
}

function useDeployModel(t: Translate) {
  const [targetPath, setTargetPath] = useState("");
  const selection = useBackupSelection();
  const targets = useTargetProfiles();
  const actions = useDeployActions(t, selection.repositoryId, selection.backupId, targets.targetProfileId, targetPath);
  return { ...selection, ...targets, targetPath, setTargetPath, ...actions };
}

type DeployModel = ReturnType<typeof useDeployModel>;

function DeployForm({ model, t }: { model: DeployModel; t: Translate }) {
  if (model.repositories.length === 0) {
    return <p className="restore-panel__empty">{t("restoreNoRepositories")}</p>;
  }
  return (
    <form className="repository-form" onSubmit={model.preview}>
      <RepositoryField model={model} t={t} />
      <BackupField model={model} t={t} />
      <TargetProfileField model={model} t={t} />
      <TargetPathField model={model} t={t} />
      <div className="repository-form__actions">
        <PreviewButton model={model} t={t} />
      </div>
    </form>
  );
}

function RepositoryField({ model, t }: { model: DeployModel; t: Translate }) {
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

function BackupField({ model, t }: { model: DeployModel; t: Translate }) {
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
          <option key={backup.backupId} value={backup.backupId}>{backup.backupId} — {backup.sealedAt}</option>
        ))}
      </select>
    </label>
  );
}

function TargetProfileField({ model, t }: { model: DeployModel; t: Translate }) {
  if (model.targetProfiles.length === 0) {
    return (
      <label>
        <span>{t("deployTargetProfile")}</span>
        <span className="restore-panel__empty">{t("deployNoTargetProfiles")}</span>
      </label>
    );
  }
  return (
    <label>
      <span>{t("deployTargetProfile")}</span>
      <select value={model.targetProfileId} onChange={(event) => model.setTargetProfileId(event.target.value)} required>
        {model.targetProfiles.map((profile) => (
          <option key={profile.profileId} value={profile.profileId}>{profile.label} · {profile.user}@{profile.host}:{profile.port}</option>
        ))}
      </select>
    </label>
  );
}

function TargetPathField({ model, t }: { model: DeployModel; t: Translate }) {
  return (
    <label className="repository-form__actions">
      <span>{t("deployTargetPath")}</span>
      <input
        value={model.targetPath}
        onChange={(event) => model.setTargetPath(event.target.value)}
        placeholder={t("deployTargetPathHint")}
        required
      />
    </label>
  );
}

function PreviewButton({ model, t }: { model: DeployModel; t: Translate }) {
  const disabled = !model.repositoryId || !model.backupId || !model.targetProfileId || !model.targetPath || model.previewing;
  return (
    <button className="button button--primary" type="submit" disabled={disabled}>
      {model.previewing ? <LoaderCircle className="spin" size={16} /> : <Eye size={16} />}
      {model.previewing ? t("deployPreviewing") : t("deployPreview")}
    </button>
  );
}

function DeployConfirmation({ model, t }: { model: DeployModel; t: Translate }) {
  const plan = model.plan;
  if (!plan) return null;
  const confirmed = model.confirmationInput === plan.confirmation;
  return (
    <div className="signing-confirm" aria-live="polite">
      <div>
        <strong>{t("deployPlanTitle")}</strong>
        <p>{t("deployPlanTarget")}: {plan.targetProfileLabel} · {plan.targetPath}</p>
        <p>{t("deployPlanPayload")}: {plan.filesystemPayload}</p>
        {plan.databasePayload && <p>{t("deployPlanDatabasePayload")}: {plan.databasePayload}</p>}
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
      <p>{t("deployPlanConfirmHint")}</p>
      <div className="signing-confirm__actions">
        <button className="button button--secondary" disabled={model.deploying} onClick={model.cancelPlan} type="button">
          {t("restoreCancel")}
        </button>
        <button className="button button--primary" disabled={!confirmed || model.deploying} onClick={model.deploy} type="button">
          {model.deploying ? <LoaderCircle className="spin" size={16} /> : <Rocket size={16} />}
          {model.deploying ? t("deployExecuting") : t("deployExecute")}
        </button>
        {model.deploying && (
          <button className="button button--secondary" onClick={model.cancelRunningDeploy} type="button">
            {t("deployCancelRunning")}
          </button>
        )}
      </div>
    </div>
  );
}

function errorText(error: unknown, t: Translate): string {
  return isDeployFailure(error) ? `${error.message} ${error.remediation}` : t("deployErrorFallback");
}

function isDeployFailure(error: unknown): error is DeployFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error
    && typeof error.message === "string" && typeof error.remediation === "string";
}
