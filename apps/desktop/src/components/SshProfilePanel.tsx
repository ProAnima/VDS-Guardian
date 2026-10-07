import { Check, CircleAlert, Plus, Server, X } from "lucide-react";
import type { Translate } from "../i18n";
import { tip } from "../shared/tip";
import { EmptyNotice } from "./EmptyNotice";
import { ResourceLoadFailure } from "./ResourceLoadFailure";
import { ServerForm } from "./servers/ServerForm";
import { ServerList } from "./servers/ServerList";
import { useServers, type ServersModel } from "./servers/useServers";

export function SshProfilePanel({ onProfilesChanged, refreshKey = 0, t }: { onProfilesChanged: () => void; refreshKey?: number; t: Translate }) {
  const model = useServers(onProfilesChanged, t, refreshKey);
  const { list } = model;
  return (
    <div className="servers-view">
      <div className="backup-toolbar" role="toolbar" aria-label={t("serverManagerTitle")}>
        <button className="icon-button icon-button--large" type="button" disabled={list.loading || Boolean(list.loadFailure) || model.formOpen} onClick={() => model.setFormOpen(true)} {...tip(t("serversAdd"))}><Plus size={17} aria-hidden="true" /></button>
      </div>
      <div className="servers-view__body"><Body model={model} t={t} /></div>
      <Notices model={model} t={t} />
      {model.formOpen && !list.loading && !list.loadFailure && <ServerForm model={model} t={t} />}
    </div>
  );
}

function Body({ model, t }: { model: ServersModel; t: Translate }) {
  const { list } = model;
  if (list.loadFailure) return <ResourceLoadFailure message={list.loadFailure} onRetry={() => void list.refresh()} retryLabel={t("readinessRefresh")} retrying={list.loading} />;
  if (list.loading) return <p className="empty-notice">{t("readinessLoading")}</p>;
  if (list.profiles.length === 0) return <EmptyNotice icon={Server} message={t("serversEmpty")} />;
  return <ServerList model={model} t={t} />;
}

function Notices({ model, t }: { model: ServersModel; t: Translate }) {
  if (!model.result && !model.failure) return null;
  return (
    <div className="backup-feedback">
      {model.result && <p className="backup-feedback__ok" role="status"><Check size={15} aria-hidden="true" />{model.result}<button className="icon-button" type="button" onClick={model.dismiss} {...tip(t("dismiss"))}><X size={13} aria-hidden="true" /></button></p>}
      {model.failure && <p className="servers-view__error" role="alert"><CircleAlert size={15} aria-hidden="true" />{model.failure}</p>}
    </div>
  );
}
