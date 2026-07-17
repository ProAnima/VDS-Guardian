import { useEffect, useState } from "react";
import { CircleAlert, CircleCheck, LoaderCircle, RefreshCw } from "lucide-react";
import {
  getSigningIdentityStatus, listCapturePlans, listRepositories, listSshProfiles,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import { evaluateSetupReadiness, type SetupResources, type SetupStatusItem } from "./setup-readiness";
import type { Translate } from "../i18n";
interface LoadFailure { label: string; detail: string; }

export function SetupStatusPanel({ resourcesRevision, t }: { resourcesRevision: number; t: Translate }) {
  const model = useSetupStatus(resourcesRevision, t);
  return <section className="setup-status" aria-labelledby="setup-status-title">
    <header><div><p className="eyebrow">{t("readinessEyebrow")}</p><h2 id="setup-status-title">{t("readinessTitle")}</h2><p>{t("readinessBody")}</p></div><button className="text-button" disabled={model.loading} onClick={model.reload} type="button"><RefreshCw size={15} />{t("readinessRefresh")}</button></header>
    {model.loading && !model.resources && <p className="setup-status__loading"><LoaderCircle className="spin" size={16} />{t("readinessLoading")}</p>}
    {model.resources && <div className="setup-status__items">{evaluateSetupReadiness(model.resources, t).map((item) => <StatusItem key={item.label} item={item} />)}</div>}
    {model.failures.length > 0 && <div className="setup-status__failures" role="alert">{model.failures.map((failure) => <p key={failure.label}><CircleAlert size={16} />{t("readinessFailurePrefix")} «{failure.label}»: {failure.detail}</p>)}</div>}
  </section>;
}

function StatusItem({ item }: { item: SetupStatusItem }) {
  const Icon = item.readiness === "ready" ? CircleCheck : CircleAlert;
  return <div data-ready={item.readiness === "ready" || undefined}><Icon size={16} /><div><strong>{item.label}</strong><span>{item.detail}</span></div></div>;
}

function useSetupStatus(resourcesRevision: number, t: Translate) {
  const [resources, setResources] = useState<SetupResources>();
  const [failures, setFailures] = useState<LoadFailure[]>([]);
  const [loading, setLoading] = useState(true);
  const [reloadRevision, setReloadRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void loadSetupStatus(t, (result) => { if (active) { setResources(result.resources); setFailures(result.failures); setLoading(false); } });
    return () => { active = false; };
  }, [resourcesRevision, reloadRevision, t]);
  return { resources, failures, loading, reload: () => { setLoading(true); setReloadRevision((value) => value + 1); } };
}

async function loadSetupStatus(t: Translate, update: (result: { resources?: SetupResources; failures: LoadFailure[] }) => void) {
  const [identity, repositories, profiles, plans] = await Promise.allSettled([getSigningIdentityStatus(), listRepositories(), listSshProfiles(), listCapturePlans()]);
  const failures = [
    loadFailure(t("readinessIdentity"), identity, t), loadFailure(t("readinessRepositoriesResource"), repositories, t),
    loadFailure(t("readinessServersResource"), profiles, t), loadFailure(t("readinessPlansResource"), plans, t),
  ].flatMap((failure) => failure ? [failure] : []);
  update({ resources: {
    identity: identity.status === "fulfilled" ? identity.value : undefined,
    repositories: repositories.status === "fulfilled" ? repositories.value : undefined,
    profiles: profiles.status === "fulfilled" ? profiles.value : undefined,
    plans: plans.status === "fulfilled" ? plans.value : undefined,
  }, failures });
}

function loadFailure(label: string, result: PromiseSettledResult<unknown>, t: Translate): LoadFailure | undefined { return result.status === "rejected" ? { label, detail: errorText(result.reason, t) } : undefined; }
function errorText(error: unknown, t: Translate): string { return safeErrorText(error, t("readinessErrorFallback")); }
