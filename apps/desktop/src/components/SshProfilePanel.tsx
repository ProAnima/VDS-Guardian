import { useCallback, useEffect, useState, type FormEvent } from "react";
import { CircleAlert, CircleCheck, KeyRound, LoaderCircle, Server, Wifi } from "lucide-react";
import {
  enrollSshProfile, hasTauriRuntime, listSshProfiles, pickSshKeyPath, preflightSshProfile, testSshProfile,
  type SshProfileFailure, type SshProfileRequest, type SshProfileSummary,
} from "../shared/commands";

const initialForm: SshProfileRequest = { label: "", host: "", port: 22, user: "", hostKey: "", keyPath: "" };

export function SshProfilePanel() {
  const model = useSshProfile();
  return <section className="ssh-profile-panel" aria-labelledby="ssh-profile-title">
    <header className="ssh-profile-panel__header"><div><p className="eyebrow"><Server size={15} aria-hidden="true" />Сервер</p><h2 id="ssh-profile-title">Добавить сервер</h2><p>Один SSH-профиль. Ключ сохранится только в хранилище учётных данных ОС.</p></div><span className="signing-state"><Wifi size={16} />SSH</span></header>
    <form className="ssh-profile-form" onSubmit={(event) => void model.submit(event)}>
      <label><span>Название</span><input value={model.form.label} onChange={(event) => model.setForm({ ...model.form, label: event.target.value })} placeholder="Production VDS" required maxLength={128} /></label>
      <label><span>Адрес сервера</span><input value={model.form.host} onChange={(event) => model.setForm({ ...model.form, host: event.target.value })} placeholder="vds.example.com" required /></label>
      <label><span>SSH-пользователь</span><input value={model.form.user} onChange={(event) => model.setForm({ ...model.form, user: event.target.value })} placeholder="backup" required /></label>
      <label><span>Порт</span><input value={model.form.port} onChange={(event) => model.setForm({ ...model.form, port: Number(event.target.value) })} type="number" min={1} max={65535} required /></label>
      <label className="ssh-profile-form__wide"><span>Проверенный host key</span><input value={model.form.hostKey} onChange={(event) => model.setForm({ ...model.form, hostKey: event.target.value })} placeholder="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI…" required /></label>
      <label className="ssh-profile-form__wide"><span>SSH-ключ</span><span className="path-picker"><input value={model.form.keyPath} onChange={(event) => model.setForm({ ...model.form, keyPath: event.target.value })} placeholder="Выберите файл ключа" required /><button type="button" onClick={() => void pickSshKeyPath().then((path) => path && model.setForm({ ...model.form, keyPath: path }))}>Обзор…</button></span></label>
      <label className="ssh-profile-form__ack"><input checked={model.acknowledged} onChange={(event) => model.setAcknowledged(event.target.checked)} type="checkbox" />Я проверил host key другим доверенным способом.</label>
      <div className="ssh-profile-form__actions"><button className="button button--primary" disabled={!model.acknowledged || model.working || !hasTauriRuntime()} type="submit">{model.working ? <LoaderCircle className="spin" size={16} /> : <KeyRound size={16} />}{model.working ? "Сохраняем и проверяем…" : "Сохранить и проверить"}</button>{!hasTauriRuntime() && <span className="signing-panel__desktop">Доступно в desktop-приложении</span>}</div>
    </form>
    {model.failure && <p className="signing-panel__error" role="alert"><CircleAlert size={16} />{model.failure}</p>}
    {model.result && <p className="ssh-profile-panel__success"><CircleCheck size={16} />{model.result}</p>}
    {model.profiles.length > 0 && <div className="ssh-profile-panel__profiles">{model.profiles.map((profile) => <span key={profile.profileId}>{profile.label} · {profile.user}@{profile.host}:{profile.port}</span>)}</div>}
  </section>;
}

function useSshProfile() {
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [form, setForm] = useState(initialForm);
  const [acknowledged, setAcknowledged] = useState(false);
  const [working, setWorking] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const refresh = useCallback(async () => {
    try { setProfiles(await listSshProfiles()); } catch (error) { setFailure(errorText(error)); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!acknowledged || !hasTauriRuntime()) return;
    setWorking(true); setFailure(undefined); setResult(undefined);
    try {
      const profile = await enrollSshProfile(form);
      await testSshProfile(profile.profileId);
      await preflightSshProfile(profile.profileId);
      setProfiles((current) => [...current, profile]);
      setForm(initialForm); setAcknowledged(false);
      setResult(`Сервер «${profile.label}» добавлен: SSH и tar.zstd для будущего backup подтверждены.`);
    } catch (error) { setFailure(errorText(error)); } finally { setWorking(false); }
  };
  return { profiles, form, acknowledged, working, result, failure, setForm, setAcknowledged, submit };
}

function errorText(error: unknown): string {
  if (isProfileFailure(error)) return `${error.message} ${error.remediation}`;
  return "Не удалось безопасно завершить настройку SSH-профиля.";
}

function isProfileFailure(error: unknown): error is SshProfileFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error && typeof error.message === "string" && typeof error.remediation === "string";
}
