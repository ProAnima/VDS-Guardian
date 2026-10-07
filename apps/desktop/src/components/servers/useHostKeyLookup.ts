import { useState } from "react";
import type { Translate } from "../../i18n";
import { scanHostKey, type ScannedHostKey, type SshProfileRequest } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";

/**
 * Fetching the server's host key is only a convenience; it never confirms trust. Every new key
 * resets the operator's acknowledgement, and changing the address or port discards a fetched key
 * so a key can never be paired with a different server than the one it came from.
 */
export function useHostKeyLookup(
  form: SshProfileRequest,
  setForm: (form: SshProfileRequest) => void,
  setAcknowledged: (value: boolean) => void,
  t: Translate,
) {
  const [fetched, setFetched] = useState<ScannedHostKey>();
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string>();
  const forget = () => { if (fetched) { setFetched(undefined); setAcknowledged(false); } };
  const lookup = async () => {
    setBusy(true); setFailure(undefined);
    try {
      const key = await scanHostKey(form.host.trim(), form.port);
      setFetched(key); setAcknowledged(false);
      setForm({ ...form, hostKey: `${key.algorithm} ${key.publicKey}` });
    } catch (error) { setFailure(safeErrorText(error, t("setupFetchFailed"))); }
    finally { setBusy(false); }
  };
  /** Edits to the address or port invalidate a fetched key; any other edit just applies. */
  const editAddress = (patch: Partial<SshProfileRequest>) => {
    if (fetched) { forget(); setForm({ ...form, ...patch, hostKey: "" }); } else setForm({ ...form, ...patch });
    setFailure(undefined);
  };
  const editHostKey = (hostKey: string) => { forget(); setForm({ ...form, hostKey }); };
  return { fetched, busy, failure, lookup, editAddress, editHostKey };
}

export type HostKeyLookup = ReturnType<typeof useHostKeyLookup>;
