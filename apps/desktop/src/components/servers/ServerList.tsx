import { Check, KeyRound, LoaderCircle, Server, Trash2, X } from "lucide-react";
import type { Translate } from "../../i18n";
import type { SshProfileSummary } from "../../shared/commands";
import { tip } from "../../shared/tip";
import type { ServersModel } from "./useServers";

export function ServerList({ model, t }: { model: ServersModel; t: Translate }) {
  const { list } = model;
  return (
    <ul className="server-rows" aria-label={t("serverManagerTitle")}>
      {list.profiles.map((profile) => <ServerRow key={profile.profileId} profile={profile} model={model} t={t} />)}
    </ul>
  );
}

function ServerRow({ profile, model, t }: { profile: SshProfileSummary; model: ServersModel; t: Translate }) {
  const { list } = model;
  const confirming = list.confirmingId === profile.profileId;
  const deleting = list.deletingId === profile.profileId;
  return (
    <li className="server-row" data-confirming={confirming || undefined}>
      <span className="server-row__icon"><Server size={17} aria-hidden="true" /></span>
      <strong>{profile.label}</strong>
      <code>{profile.user}@{profile.host}:{profile.port}</code>
      <span className="server-row__auth" {...tip(t("serversSshKey"))}><KeyRound size={14} aria-hidden="true" /></span>
      {confirming ? (
        <span className="server-row__confirm" role="group" aria-label={t("serversDeleteQuestion")}>
          <span>{t("serversDeleteQuestion")}</span>
          <button className="icon-button" type="button" disabled={deleting} onClick={() => list.setConfirmingId(undefined)} {...tip(t("serversCancel"))}><X size={14} aria-hidden="true" /></button>
          <button className="icon-button server-row__danger" type="button" disabled={deleting} onClick={() => void list.remove(profile)} {...tip(t("serversDelete"))}>
            {deleting ? <LoaderCircle className="spin" size={14} aria-hidden="true" /> : <Check size={14} aria-hidden="true" />}
          </button>
        </span>
      ) : (
        <button className="icon-button server-row__trash" type="button" onClick={() => list.setConfirmingId(profile.profileId)} {...tip(`${t("serversDelete")} ${profile.label}`)}><Trash2 size={15} aria-hidden="true" /></button>
      )}
    </li>
  );
}
