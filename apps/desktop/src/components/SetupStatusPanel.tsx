import { useEffect, useState } from "react";
import { CircleAlert, CircleCheck, LoaderCircle, RefreshCw } from "lucide-react";
import {
  getSigningIdentityStatus, listCapturePlans, listRepositories, listSshProfiles,
} from "../shared/commands";
import { evaluateSetupReadiness, type SetupResources, type SetupStatusItem } from "./setup-readiness";
interface LoadFailure { label: string; detail: string; }

export function SetupStatusPanel({ resourcesRevision }: { resourcesRevision: number }) {
  const model = useSetupStatus(resourcesRevision);
  return <section className="setup-status" aria-labelledby="setup-status-title">
    <header><div><p className="eyebrow">Готовность</p><h2 id="setup-status-title">Статус настройки</h2><p>Перед запуском первого бэкапа проверьте все обязательные шаги в одном месте.</p></div><button className="text-button" disabled={model.loading} onClick={model.reload} type="button"><RefreshCw size={15} />Обновить</button></header>
    {model.loading && !model.resources && <p className="setup-status__loading"><LoaderCircle className="spin" size={16} />Проверяем локальную настройку…</p>}
    {model.resources && <div className="setup-status__items">{evaluateSetupReadiness(model.resources).map((item) => <StatusItem key={item.label} item={item} />)}</div>}
    {model.failures.length > 0 && <div className="setup-status__failures" role="alert">{model.failures.map((failure) => <p key={failure.label}><CircleAlert size={16} />Не удалось проверить «{failure.label}»: {failure.detail}</p>)}</div>}
  </section>;
}

function StatusItem({ item }: { item: SetupStatusItem }) {
  const Icon = item.readiness === "ready" ? CircleCheck : CircleAlert;
  return <div data-ready={item.readiness === "ready" || undefined}><Icon size={16} /><div><strong>{item.label}</strong><span>{item.detail}</span></div></div>;
}

function useSetupStatus(resourcesRevision: number) {
  const [resources, setResources] = useState<SetupResources>();
  const [failures, setFailures] = useState<LoadFailure[]>([]);
  const [loading, setLoading] = useState(true);
  const [reloadRevision, setReloadRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void loadSetupStatus((result) => { if (active) { setResources(result.resources); setFailures(result.failures); setLoading(false); } });
    return () => { active = false; };
  }, [resourcesRevision, reloadRevision]);
  return { resources, failures, loading, reload: () => { setLoading(true); setReloadRevision((value) => value + 1); } };
}

async function loadSetupStatus(update: (result: { resources?: SetupResources; failures: LoadFailure[] }) => void) {
  const [identity, repositories, profiles, plans] = await Promise.allSettled([getSigningIdentityStatus(), listRepositories(), listSshProfiles(), listCapturePlans()]);
  const failures = [
    loadFailure("подписывающую идентичность", identity), loadFailure("хранилища", repositories),
    loadFailure("SSH-серверы", profiles), loadFailure("планы бэкапа", plans),
  ].flatMap((failure) => failure ? [failure] : []);
  update({ resources: {
    identity: identity.status === "fulfilled" ? identity.value : undefined,
    repositories: repositories.status === "fulfilled" ? repositories.value : undefined,
    profiles: profiles.status === "fulfilled" ? profiles.value : undefined,
    plans: plans.status === "fulfilled" ? plans.value : undefined,
  }, failures });
}

function loadFailure(label: string, result: PromiseSettledResult<unknown>): LoadFailure | undefined { return result.status === "rejected" ? { label, detail: errorText(result.reason) } : undefined; }
function errorText(error: unknown): string { return error instanceof Error ? error.message : "Повторите проверку; если ошибка сохранится, откройте диагностику."; }
