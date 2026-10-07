import { useCallback, useEffect, useState, type FormEvent } from "react";
import type { Translate } from "../../i18n";
import {
  deleteSshProfile, enrollSshProfile, hasTauriRuntime, listSshProfiles,
  type SshProfileRequest, type SshProfileSummary,
} from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";

export const initialServerForm: SshProfileRequest = { label: "", host: "", port: 22, user: "", hostKey: "", keyPath: "" };

/** Saved servers: loading, retry and confirmed removal. */
function useServerList(t: Translate, notify: (message: { result?: string; failure?: string }) => void, onChanged: () => void) {
  const [profiles, setProfiles] = useState<SshProfileSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailure, setLoadFailure] = useState<string>();
  const [confirmingId, setConfirmingId] = useState<string>();
  const [deletingId, setDeletingId] = useState<string>();
  const refresh = useCallback(async () => {
    setLoading(true); setLoadFailure(undefined);
    try { const next = await listSshProfiles(); setProfiles(next); return next; }
    catch (error) { setLoadFailure(safeErrorText(error, t("setupServerError"))); return undefined; }
    finally { setLoading(false); }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);
  const remove = async (profile: SshProfileSummary) => {
    setDeletingId(profile.profileId);
    try {
      await deleteSshProfile(profile.profileId); await refresh();
      setConfirmingId(undefined); onChanged(); notify({ result: `${t("serversDeleted")} ${profile.label}` });
    } catch (error) { notify({ failure: safeErrorText(error, t("setupServerError")) }); }
    finally { setDeletingId(undefined); }
  };
  return { profiles, setProfiles, loading, loadFailure, confirmingId, setConfirmingId, deletingId, refresh, remove };
}

/** The add-server form and its single enrollment transaction. */
function useServerEnrollment(t: Translate, onEnrolled: (profile: SshProfileSummary) => void, notify: (message: { result?: string; failure?: string }) => void) {
  const [form, setForm] = useState(initialServerForm);
  const [acknowledged, setAcknowledged] = useState(false);
  const [working, setWorking] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!acknowledged || !hasTauriRuntime()) return;
    setWorking(true);
    try {
      const profile = await enrollSshProfile(form);
      onEnrolled(profile); setForm(initialServerForm); setAcknowledged(false);
      notify({ result: `${t("setupServerCreated")} ${profile.label}` });
    } catch (error) { notify({ failure: safeErrorText(error, t("setupServerError")) }); }
    finally { setWorking(false); }
  };
  return { form, setForm, acknowledged, setAcknowledged, working, submit };
}

export function useServers(onProfilesChanged: () => void, t: Translate) {
  const [formOpen, setFormOpen] = useState(false);
  const [result, setResult] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const notify = (message: { result?: string; failure?: string }) => { setResult(message.result); setFailure(message.failure); };
  const list = useServerList(t, notify, onProfilesChanged);
  const enrollment = useServerEnrollment(t, (profile) => {
    list.setProfiles((current) => [...current, profile]); onProfilesChanged(); setFormOpen(false);
  }, notify);
  const { loading, loadFailure, profiles } = list;
  // With no saved server the next step is obvious, so open the form once the list is known to be empty.
  useEffect(() => { if (!loading && !loadFailure && profiles.length === 0) setFormOpen(true); }, [loading, loadFailure, profiles.length]);
  return { list, enrollment, formOpen, setFormOpen, result, failure, dismiss: () => { setResult(undefined); setFailure(undefined); } };
}

export type ServersModel = ReturnType<typeof useServers>;
