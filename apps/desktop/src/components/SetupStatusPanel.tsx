import { ArrowUpRight, CircleAlert, CircleCheck, LoaderCircle, RefreshCw } from "lucide-react";
import { evaluateSetupReadiness, type SetupStatusItem } from "./setup-readiness";
import { useSetupStatus } from "./use-setup-status";
import type { Translate } from "../i18n";

interface SetupStatusPanelProps {
  onManageServers?: () => void;
  onOpenSettings?: (target: "protection" | "storage") => void;
  resourcesRevision: number;
  t: Translate;
}

export function SetupStatusPanel({ onManageServers, onOpenSettings, resourcesRevision, t }: SetupStatusPanelProps) {
  const model = useSetupStatus(resourcesRevision, t);
  const items = model.resources ? evaluateSetupReadiness(model.resources, t) : [];
  const ready = items.length > 0 && items.every((item) => item.readiness === "ready");
  return <section className="setup-status" aria-labelledby="setup-status-title">
    <header><h2 id="setup-status-title">{ready && <CircleCheck size={16} />}{t(ready ? "backupReadyTitle" : "backupChecklistTitle")}</h2><button aria-label={t("readinessRefresh")} className="text-button" disabled={model.loading} title={t("readinessRefresh")} onClick={model.reload} type="button"><RefreshCw className={model.loading ? "spin" : undefined} size={15} /></button></header>
    {model.loading && !model.resources && <p className="setup-status__loading"><LoaderCircle className="spin" size={16} />{t("readinessLoading")}</p>}
    {!ready && items.length > 0 && <div className="setup-status__items">{items.map((item) => <StatusItem key={item.label} item={item} onClick={statusAction(item, onManageServers, onOpenSettings)} />)}</div>}
    {model.failures.length > 0 && <div className="setup-status__failures" role="alert">{model.failures.map((failure) => <p key={failure.label}><CircleAlert size={16} />{t("readinessFailurePrefix")} «{failure.label}»: {failure.detail}</p>)}</div>}
  </section>;
}

function StatusItem({ item, onClick }: { item: SetupStatusItem; onClick?: () => void }) {
  const Icon = item.readiness === "ready" ? CircleCheck : CircleAlert;
  if (item.readiness === "ready") return <div data-ready><Icon size={16} /><div><strong>{item.label}</strong><span>{item.detail}</span></div></div>;
  return <button type="button" disabled={!onClick} onClick={onClick}><Icon size={16} /><span><strong>{item.label}</strong><small>{item.detail}</small></span><ArrowUpRight size={14} /></button>;
}

function statusAction(item: SetupStatusItem, servers?: () => void, settings?: (target: "protection" | "storage") => void): (() => void) | undefined {
  if (item.target === "servers") return servers;
  const target = item.target;
  return settings ? () => settings(target) : undefined;
}
