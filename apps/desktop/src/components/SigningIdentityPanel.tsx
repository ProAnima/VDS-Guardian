import { useCallback, useEffect, useState } from "react";
import { CircleAlert, CircleCheck, Fingerprint, KeyRound, LoaderCircle, RefreshCw, ShieldCheck } from "lucide-react";
import type { Translate } from "../i18n";
import {
  enrollSigningIdentity, getSigningIdentityStatus, hasTauriRuntime,
  type SigningIdentityState, type SigningIdentityStatus,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";

interface SigningIdentityPanelProps { onIdentityChanged: () => void; t: Translate; }

export function SigningIdentityPanel({ onIdentityChanged, t }: SigningIdentityPanelProps) {
  const model = useSigningIdentity(t, onIdentityChanged);
  return (
    <section className="signing-panel" aria-labelledby="signing-identity-title">
      <SigningHeader state={model.status?.state} t={t} />
      <IdentityDetails status={model.status} t={t} />
      {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
      <SigningActions model={model} t={t} />
    </section>
  );
}

function useSigningIdentity(t: Translate, onIdentityChanged: () => void) {
  const [status, setStatus] = useState<SigningIdentityStatus>();
  const [failure, setFailure] = useState<string>();
  const [confirming, setConfirming] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [enrolling, setEnrolling] = useState(false);
  const refresh = useStatusLoader(t, setStatus, setFailure);
  useEffect(() => { void refresh(); }, [refresh]);
  const submit = useEnrollment(t, onIdentityChanged, setStatus, setFailure, setConfirming, setAcknowledged, setEnrolling);
  return { status, failure, confirming, acknowledged, enrolling, refresh, submit, setConfirming, setAcknowledged };
}

function useStatusLoader(t: Translate, setStatus: (value: SigningIdentityStatus) => void, setFailure: (value: string | undefined) => void) {
  return useCallback(async () => {
    setFailure(undefined);
    try { setStatus(await getSigningIdentityStatus()); } catch (error) { setFailure(errorText(error, t)); }
  }, [setFailure, setStatus, t]);
}

function useEnrollment(t: Translate, onIdentityChanged: () => void, setStatus: (value: SigningIdentityStatus) => void, setFailure: (value: string | undefined) => void, setConfirming: (value: boolean) => void, setAcknowledged: (value: boolean) => void, setEnrolling: (value: boolean) => void) {
  return useCallback(async () => {
    setEnrolling(true);
    setFailure(undefined);
    try {
      const enrolled = await enrollSigningIdentity();
      setStatus({ state: "ready", identity: enrolled.identity });
      setConfirming(false); setAcknowledged(false);
      onIdentityChanged();
    } catch (error) { setFailure(errorText(error, t)); } finally { setEnrolling(false); }
  }, [onIdentityChanged, setAcknowledged, setConfirming, setEnrolling, setFailure, setStatus, t]);
}

function SigningHeader({ state, t }: { state: SigningIdentityState | undefined; t: Translate }) {
  const label = state ? t(stateLabel(state)) : t("signingLoading");
  const ready = state === "ready";
  return <header className="signing-panel__header"><div><p className="eyebrow"><ShieldCheck size={15} aria-hidden="true" />{t("signingEyebrow")}</p><h2 id="signing-identity-title">{t("signingTitle")}</h2><p>{t("signingBody")}</p></div><span className="signing-state" data-ready={ready || undefined}>{ready ? <CircleCheck size={16} /> : <Fingerprint size={16} />}{label}</span></header>;
}

function IdentityDetails({ status, t }: { status: SigningIdentityStatus | undefined; t: Translate }) {
  if (!status?.identity) return null;
  return <dl className="signing-details"><div><dt>{t("signingCredential")}</dt><dd>{status.identity.credentialId}</dd></div><div><dt>{t("signingKeyId")}</dt><dd>{status.identity.keyId}</dd></div></dl>;
}

type SigningModel = ReturnType<typeof useSigningIdentity>;

function SigningActions({ model, t }: { model: SigningModel; t: Translate }) {
  if (model.confirming) return <EnrollmentConfirmation model={model} t={t} />;
  const canEnroll = model.status !== undefined && model.status.state !== "ready" && hasTauriRuntime();
  return <div className="signing-panel__actions"><button className="text-button" onClick={() => void model.refresh()} type="button"><RefreshCw size={15} />{t("signingRefresh")}</button>{canEnroll && <button className="button button--primary" onClick={() => model.setConfirming(true)} type="button"><KeyRound size={16} />{model.status?.state === "not_enrolled" ? t("signingStart") : t("signingFinish")}</button>}{!hasTauriRuntime() && <span className="signing-panel__desktop">{t("signingDesktopRequired")}</span>}</div>;
}

function EnrollmentConfirmation({ model, t }: { model: SigningModel; t: Translate }) {
  return <div className="signing-confirm" aria-live="polite"><div><strong>{t("signingConfirmTitle")}</strong><p>{t("signingConfirmBody")}</p></div><label><input checked={model.acknowledged} onChange={(event) => model.setAcknowledged(event.target.checked)} type="checkbox" />{t("signingAcknowledge")}</label><div className="signing-confirm__actions"><button className="button button--secondary" disabled={model.enrolling} onClick={() => { model.setConfirming(false); model.setAcknowledged(false); }} type="button">{t("signingCancel")}</button><button className="button button--primary" disabled={!model.acknowledged || model.enrolling} onClick={() => void model.submit()} type="button">{model.enrolling ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{model.enrolling ? t("signingCreating") : t("signingCreate")}</button></div></div>;
}

function stateLabel(state: SigningIdentityState) {
  return ({ not_enrolled: "signingNotEnrolled", enrollment_pending: "signingEnrollmentPending", recovery_pending: "signingRecoveryPending", ready: "signingReady" } as const)[state];
}

function errorText(error: unknown, t: Translate): string {
  return safeErrorText(error, t("signingErrorFallback"));
}
