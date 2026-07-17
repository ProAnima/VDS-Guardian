import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, KeyRound, LoaderCircle, Plus, Server, ShieldCheck, Trash2, Wifi } from "lucide-react";
import {
  deleteSshProfile, enrollSshProfile, hasTauriRuntime, listSshProfiles, pickSshKeyPath,
  type SshProfileRequest, type SshProfileSummary,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";
import type { Translate } from "../i18n";

const initialForm: SshProfileRequest = { label: "", host: "", port: 22, user: "", hostKey: "", keyPath: "" };

export function SshProfilePanel({ onProfilesChanged, t }: { onProfilesChanged: () => void; t: Translate }) {
  const model = useSshProfile(onProfilesChanged, t);
  return <section className="ssh-profile-panel" aria-labelledby="ssh-profile-title">
    <header className="ssh-profile-panel__header"><div><p className="eyebrow"><Server size={15} aria-hidden="true" />{t("setupServerEyebrow")}</p><h2 id="ssh-profile-title">{t("serverManagerTitle")}</h2><p>{t("serversBody")}</p></div><span className="signing-state"><Wifi size={16} />SSH</span></header>
    <ServerCards model={model} t={t} />
    {!model.formOpen && <button className="button button--secondary ssh-profile-panel__add" type="button" onClick={() => model.setFormOpen(true)}><Plus size={16} />{t("serversAdd")}</button>}
    {model.formOpen && <ServerForm model={model} t={t} />}
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="ssh-profile-panel__success"><CircleCheck size={16} />{model.result}</p>}
  </section>;
}

function ServerCards({ model, t }: { model: SshProfileModel; t: Translate }) {
  if (model.profiles.length === 0) return <p className="server-list__empty">{t("serversEmpty")}</p>;
  return <div className="server-list">{model.profiles.map((profile) => <article className="server-card" key={profile.profileId}>
    <div className="server-card__icon"><Server size={19} /></div>
    <div className="server-card__main"><strong>{profile.label}</strong><span>{profile.user}@{profile.host}:{profile.port}</span><small><ShieldCheck size={13} />{t("serversSshKey")}</small></div>
    {model.confirmingId === profile.profileId
      ? <div className="server-card__confirm"><span>{t("serversDeleteQuestion")}</span><button type="button" onClick={() => model.setConfirmingId(undefined)}>{t("serversCancel")}</button><button className="server-card__delete-confirm" disabled={model.deletingId === profile.profileId} type="button" onClick={() => void model.remove(profile)}>{model.deletingId === profile.profileId ? <LoaderCircle className="spin" size={14} /> : <Trash2 size={14} />}{t("serversDelete")}</button></div>
      : <button className="server-card__trash" aria-label={`${t("serversDelete")} ${profile.label}`} type="button" onClick={() => model.setConfirmingId(profile.profileId)}><Trash2 size={16} /></button>}
  </article>)}</div>;
}

function ServerForm({ model, t }: { model: SshProfileModel; t: Translate }) {
  return <form className="ssh-profile-form" onSubmit={(event) => void model.submit(event)}>
    <div className="ssh-profile-form__title"><strong>{t("setupServerTitle")}</strong>{model.profiles.length > 0 && <button type="button" onClick={() => model.setFormOpen(false)}>{t("serversCancel")}</button>}</div>
    <label><span>{t("setupLabel")}</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Production VDS" required maxLength={128} /></label>
    <label><span>{t("setupHost")}</span><input value={model.form.host} onChange={(event) => model.setForm({ ...model.form, host: event.target.value })} placeholder="vds.example.com" required /></label>
    <label><span>{t("setupUser")}</span><input value={model.form.user} onChange={(event) => model.setForm({ ...model.form, user: event.target.value })} placeholder="backup" required /></label>
    <label><span>{t("setupPort")}</span><input value={model.form.port} onChange={(event) => model.setForm({ ...model.form, port: Number(event.target.value) })} type="number" min={1} max={65535} required /></label>
    <label className="ssh-profile-form__wide"><span>{t("setupHostKey")}</span><input value={model.form.hostKey} onChange={(event) => model.setForm({ ...model.form, hostKey: event.target.value })} placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI…" required /></label>
    <label className="ssh-profile-form__wide"><span>{t("setupKey")}</span><span className="path-picker"><input value={model.form.keyPath} onChange={(event) => model.setForm({ ...model.form, keyPath: event.target.value })} placeholder={t("setupKeyPlaceholder")} required /><button type="button" onClick={() => void pickSshKeyPath().then((path) => path && model.setForm({ ...model.form, keyPath: path }))}>{t("setupBrowse")}</button></span></label>
    <label className="ssh-profile-form__ack"><input checked={model.acknowledged} onChange={(event) => model.setAcknowledged(event.target.checked)} type="checkbox" />{t("setupVerifyHostKey")}</label>
    <div className="ssh-profile-form__actions"><button className="button button--primary" disabled={!model.acknowledged || model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{model.working ? t("setupSaving") : t("setupSaveCheck")}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">{t("setupDesktopOnly")}</span>}</div>
  </form>;
}

function useSshProfile(onProfilesChanged: () => void, t: Translate) {
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [form, setForm] = useState(initialForm);
  const [formOpen, setFormOpen] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [working, setWorking] = useState(false);
  const [deletingId, setDeletingId] = useState<string>();
  const [confirmingId, setConfirmingId] = useState<string>();
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const refresh = useCallback(async () => {
    try { const next = await listSshProfiles(); setProfiles(next); setFormOpen(next.length === 0); } catch (error) { setFailure(errorText(error, t)); }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); if (!acknowledged || !hasTauriRuntime()) return;
    setWorking(true); setFailure(undefined); setResult(undefined);
    try { const profile = await enrollSshProfile(form); setProfiles((current) => [...current, profile]); onProfilesChanged(); setForm(initialForm); setAcknowledged(false); setFormOpen(false); setResult(`${t("setupServerCreated")} ${profile.label}`); }
    catch (error) { setFailure(errorText(error, t)); } finally { setWorking(false); }
  };
  const remove = async (profile: SshProfileSummary) => {
    setDeletingId(profile.profileId); setFailure(undefined); setResult(undefined);
    try { await deleteSshProfile(profile.profileId); setProfiles((current) => current.filter((item) => item.profileId !== profile.profileId)); setConfirmingId(undefined); onProfilesChanged(); setResult(`${t("serversDeleted")} ${profile.label}`); }
    catch (error) { setFailure(errorText(error, t)); } finally { setDeletingId(undefined); }
  };
  return { profiles, form, formOpen, acknowledged, working, deletingId, confirmingId, result, failure, setForm, setFormOpen, setAcknowledged, setConfirmingId, submit, remove };
}

type SshProfileModel = ReturnType<typeof useSshProfile>;

function errorText(error: unknown, t: Translate): string { return safeErrorText(error, t("setupServerError")); }
