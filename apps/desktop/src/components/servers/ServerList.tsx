import { useEffect, useRef } from "react";
import { Check, CircleHelp, KeyRound, LoaderCircle, LockKeyhole, Server, ShieldCheck, Trash2, X, type LucideIcon } from "lucide-react";
import type { Translate } from "../../i18n";
import type { AuthKind, SshProfileSummary } from "../../shared/commands";
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
  const trash = useRef<HTMLButtonElement>(null);
  const wasConfirming = useRef(false);
  // Cancelling the question puts focus back on the delete button it came from.
  useEffect(() => {
    if (wasConfirming.current && !confirming) trash.current?.focus();
    wasConfirming.current = confirming;
  }, [confirming]);
  return (
    <li className="server-row" data-confirming={confirming || undefined}>
      <span className="server-row__icon"><Server size={17} aria-hidden="true" /></span>
      <strong>{profile.label}</strong>
      <code>{profile.user}@{profile.host}:{profile.port}</code>
      <AuthBadge kind={profile.authKind} t={t} />
      {confirming ? (
        <span className="server-row__confirm" role="group" aria-label={t("serversDeleteQuestion")} aria-description={t("serversDeleteScope")}>
          <span data-tip={t("serversDeleteScope")}>{t("serversDeleteQuestion")}</span>
          <button className="icon-button" type="button" autoFocus disabled={deleting} onClick={() => list.setConfirmingId(undefined)} {...tip(t("serversCancel"))}><X size={14} aria-hidden="true" /></button>
          <button className="icon-button server-row__danger" type="button" disabled={deleting} onClick={() => void list.remove(profile)} {...tip(t("serversDelete"))}>
            {deleting ? <LoaderCircle className="spin" size={14} aria-hidden="true" /> : <Check size={14} aria-hidden="true" />}
          </button>
        </span>
      ) : (
        <button ref={trash} className="icon-button server-row__trash" type="button" onClick={() => list.setConfirmingId(profile.profileId)} {...tip(`${t("serversDelete")} ${profile.label}`)}><Trash2 size={15} aria-hidden="true" /></button>
      )}
    </li>
  );
}

const authView: Record<AuthKind, { icon: LucideIcon; label: "setupAuthKey" | "serversAuthAgent" | "setupAuthPassword" }> = {
  ssh_key: { icon: KeyRound, label: "setupAuthKey" },
  ssh_agent: { icon: ShieldCheck, label: "serversAuthAgent" },
  password: { icon: LockKeyhole, label: "setupAuthPassword" },
};

/** How this server is logged in to. Profiles saved before the kind was recorded say so. */
function AuthBadge({ kind, t }: { kind: AuthKind | undefined; t: Translate }) {
  const { icon: Icon, label } = kind ? authView[kind] : { icon: CircleHelp, label: "serversAuthUnknown" as const };
  return <span className="server-row__auth" data-kind={kind ?? "unknown"} {...tip(t(label))}><Icon size={14} aria-hidden="true" /></span>;
}
