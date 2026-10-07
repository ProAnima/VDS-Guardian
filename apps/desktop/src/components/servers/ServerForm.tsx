import type { ReactNode } from "react";
import { CircleHelp, FolderOpen, KeyRound, LoaderCircle, Server, X } from "lucide-react";
import type { Translate } from "../../i18n";
import { hasTauriRuntime, pickSshKeyPath } from "../../shared/commands";
import { tip } from "../../shared/tip";
import type { ServersModel } from "./useServers";

export function ServerForm({ model, t }: { model: ServersModel; t: Translate }) {
  const { enrollment, list } = model;
  const { form, setForm } = enrollment;
  return (
    <aside className="drawer" role="dialog" aria-label={t("setupServerTitle")}>
      <header className="drawer__header">
        <Server size={16} aria-hidden="true" /><strong>{t("setupServerTitle")}</strong>
        {list.profiles.length > 0 && <button className="icon-button" type="button" onClick={() => model.setFormOpen(false)} {...tip(t("dismiss"))}><X size={15} aria-hidden="true" /></button>}
      </header>
      <form className="server-form" onSubmit={(event) => void enrollment.submit(event)}>
        <Field label={t("setupLabel")}><input value={form.label} onChange={(event) => setForm({ ...form, label: event.target.value })} required maxLength={128} /></Field>
        <div className="server-form__pair">
          <Field label={t("setupHost")}><input value={form.host} onChange={(event) => setForm({ ...form, host: event.target.value })} placeholder="vds.example.com" required spellCheck={false} /></Field>
          <Field label={t("setupPort")} narrow><input value={form.port} onChange={(event) => setForm({ ...form, port: Number(event.target.value) })} type="number" min={1} max={65535} required /></Field>
        </div>
        <Field label={t("setupUser")}><input value={form.user} onChange={(event) => setForm({ ...form, user: event.target.value })} placeholder="backup" required spellCheck={false} /></Field>
        <Field label={t("setupHostKey")} hint={t("setupHostKeyHint")}><input value={form.hostKey} onChange={(event) => setForm({ ...form, hostKey: event.target.value })} placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI…" required spellCheck={false} /></Field>
        <Field label={t("setupKey")} hint={t("setupKeyHint")}>
          <span className="server-form__picker">
            <input value={form.keyPath} onChange={(event) => setForm({ ...form, keyPath: event.target.value })} placeholder={t("setupKeyPlaceholder")} required spellCheck={false} />
            <button className="icon-button icon-button--large" type="button" onClick={() => void pickSshKeyPath().then((path) => path && setForm({ ...form, keyPath: path }))} {...tip(t("setupBrowse"))}><FolderOpen size={16} aria-hidden="true" /></button>
          </span>
        </Field>
        <label className="server-form__ack"><input checked={enrollment.acknowledged} onChange={(event) => enrollment.setAcknowledged(event.target.checked)} type="checkbox" />{t("setupVerifyHostKey")}</label>
        <button className="button button--primary" disabled={!enrollment.acknowledged || enrollment.working || !hasTauriRuntime()} type="submit">
          {enrollment.working ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <KeyRound size={16} aria-hidden="true" />}
          {enrollment.working ? t("setupSaving") : t("serverAddCheck")}
        </button>
        {!hasTauriRuntime() && <p className="server-form__note">{t("setupDesktopOnly")}</p>}
      </form>
    </aside>
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
