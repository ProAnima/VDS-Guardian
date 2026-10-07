import { useState, type ReactNode } from "react";
import { CircleAlert, CircleHelp, Eye, EyeOff, FolderOpen, Fingerprint, KeyRound, LoaderCircle, LockKeyhole, ScanSearch, Server, X } from "lucide-react";
import type { Translate } from "../../i18n";
import { hasTauriRuntime, pickSshKeyPath, type SshProfileRequest } from "../../shared/commands";
import { tip } from "../../shared/tip";
import { useHostKeyLookup, type HostKeyLookup } from "./useHostKeyLookup";
import type { ServersModel } from "./useServers";

export function ServerForm({ model, t }: { model: ServersModel; t: Translate }) {
  const { enrollment, list } = model;
  const { form, setForm } = enrollment;
  const lookup = useHostKeyLookup(form, setForm, enrollment.setAcknowledged, t);
  // Closing the form must not leave a typed password behind in memory.
  const close = () => { setForm({ ...form, password: "" }); model.setFormOpen(false); };
  return (
    <aside className="drawer" role="dialog" aria-label={t("setupServerTitle")}>
      <header className="drawer__header">
        <Server size={16} aria-hidden="true" /><strong>{t("setupServerTitle")}</strong>
        {list.profiles.length > 0 && <button className="icon-button" type="button" onClick={close} {...tip(t("dismiss"))}><X size={15} aria-hidden="true" /></button>}
      </header>
      <form className="server-form" onSubmit={(event) => void enrollment.submit(event)}>
        <Field label={t("setupLabel")}><input value={form.label} onChange={(event) => setForm({ ...form, label: event.target.value })} required maxLength={128} /></Field>
        <div className="server-form__pair">
          <Field label={t("setupHost")}><input value={form.host} onChange={(event) => lookup.editAddress({ host: event.target.value })} placeholder="vds.example.com" required spellCheck={false} /></Field>
          <Field label={t("setupPort")} narrow><input value={form.port} onChange={(event) => lookup.editAddress({ port: Number(event.target.value) })} type="number" min={1} max={65535} required /></Field>
        </div>
        <Field label={t("setupUser")}><input value={form.user} onChange={(event) => setForm({ ...form, user: event.target.value })} placeholder="backup" required spellCheck={false} /></Field>
        <HostKeyField form={form} lookup={lookup} t={t} />
        <AuthFields form={form} setForm={setForm} t={t} />
        <label className="server-form__ack"><input checked={enrollment.acknowledged} onChange={(event) => enrollment.setAcknowledged(event.target.checked)} type="checkbox" />{t(lookup.fetched ? "setupVerifyFingerprint" : "setupVerifyHostKey")}</label>
        <button className="button button--primary" disabled={!enrollment.acknowledged || enrollment.working || !hasTauriRuntime()} type="submit">
          {enrollment.working ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <KeyRound size={16} aria-hidden="true" />}
          {enrollment.working ? t("setupSaving") : t("serverAddCheck")}
        </button>
        {!hasTauriRuntime() && <p className="server-form__note">{t("setupDesktopOnly")}</p>}
      </form>
    </aside>
  );
}

function HostKeyField({ form, lookup, t }: { form: SshProfileRequest; lookup: HostKeyLookup; t: Translate }) {
  return (
    <>
      <Field label={t("setupHostKey")} hint={t("setupHostKeyHint")}>
        <span className="server-form__picker">
          <input value={form.hostKey} onChange={(event) => lookup.editHostKey(event.target.value)} placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI…" required spellCheck={false} />
          <button className="icon-button icon-button--large" type="button" disabled={!form.host.trim() || lookup.busy || !hasTauriRuntime()} onClick={() => void lookup.lookup()} {...tip(t("setupFetchHostKey"))}>
            {lookup.busy ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <ScanSearch size={16} aria-hidden="true" />}
          </button>
        </span>
      </Field>
      {lookup.fetched && (
        <p className="server-form__fingerprint" data-tip={t("setupFingerprintHint")} data-tip-side="start">
          <Fingerprint size={15} aria-hidden="true" /><span>{t("setupFingerprint")}</span><code>{lookup.fetched.fingerprint}</code>
        </p>
      )}
      {lookup.failure && <p className="server-form__warning" role="alert"><CircleAlert size={14} aria-hidden="true" />{lookup.failure}</p>}
    </>
  );
}

interface AuthFieldsProps { form: SshProfileRequest; setForm: (form: SshProfileRequest) => void; t: Translate }

/** Choose how to log in: a key file (or the `.pub` of an agent key), or the account password. */
function AuthFields({ form, setForm, t }: AuthFieldsProps) {
  const password = form.authKind === "password";
  const choose = (authKind: SshProfileRequest["authKind"]) => setForm({ ...form, authKind, password: "" });
  return (
    <>
      <div className="server-form__modes" role="radiogroup" aria-label={t("setupServerTitle")}>
        <button type="button" role="radio" aria-checked={!password} data-active={!password || undefined} onClick={() => choose("key")} {...tip(t("setupAuthKey"))}><KeyRound size={16} aria-hidden="true" /></button>
        <button type="button" role="radio" aria-checked={password} data-active={password || undefined} onClick={() => choose("password")} {...tip(t("setupAuthPassword"))}><LockKeyhole size={16} aria-hidden="true" /></button>
      </div>
      {password ? <PasswordField form={form} setForm={setForm} t={t} /> : <KeyField form={form} setForm={setForm} t={t} />}
    </>
  );
}

function KeyField({ form, setForm, t }: AuthFieldsProps) {
  return (
    <Field label={t("setupKey")} hint={t("setupKeyHint")}>
      <span className="server-form__picker">
        <input value={form.keyPath} onChange={(event) => setForm({ ...form, keyPath: event.target.value })} placeholder={t("setupKeyPlaceholder")} required spellCheck={false} />
        <button className="icon-button icon-button--large" type="button" onClick={() => void pickSshKeyPath().then((path) => path && setForm({ ...form, keyPath: path }))} {...tip(t("setupBrowse"))}><FolderOpen size={16} aria-hidden="true" /></button>
      </span>
    </Field>
  );
}

function PasswordField({ form, setForm, t }: AuthFieldsProps) {
  const [visible, setVisible] = useState(false);
  return (
    <>
      <Field label={t("setupPassword")} hint={t("setupPasswordHint")}>
        <span className="server-form__picker">
          <input type={visible ? "text" : "password"} value={form.password} onChange={(event) => setForm({ ...form, password: event.target.value })} autoComplete="off" spellCheck={false} maxLength={256} required />
          <button className="icon-button icon-button--large" type="button" aria-pressed={visible} onClick={() => setVisible(!visible)} {...tip(t(visible ? "setupPasswordHide" : "setupPasswordShow"))}>
            {visible ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
          </button>
        </span>
      </Field>
      {form.user.trim() === "root" && <p className="server-form__warning" role="note"><CircleAlert size={14} aria-hidden="true" />{t("setupRootWarning")}</p>}
    </>
  );
}

function Field({ label, hint, narrow, children }: { label: string; hint?: string; narrow?: boolean; children: ReactNode }) {
  return (
    <label className="field" data-narrow={narrow || undefined}>
      <span className="field__label">{label}{hint && <span className="field__hint" data-tip={hint} data-tip-side="start"><CircleHelp size={13} aria-hidden="true" /></span>}</span>
      {children}
    </label>
  );
}
