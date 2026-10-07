import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { Translate } from "../../i18n";
import { scanHostKey, type ScannedHostKey, type SshProfileRequest } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";

/**
 * Fetching the server's host key is only a convenience; it never confirms trust. Any change to
 * the address, port or key withdraws the operator's confirmation, a fetched key is discarded
 * when the address or port changes, and a lookup that finishes after such a change (or after
 * the form was closed) is ignored, so a key can never be paired with another server.
 */
export function useHostKeyLookup(
  form: SshProfileRequest,
  setForm: Dispatch<SetStateAction<SshProfileRequest>>,
  setAcknowledged: (value: boolean) => void,
  t: Translate,
) {
  const [fetched, setFetched] = useState<ScannedHostKey>();
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string>();
  const generation = useRef(0);
  const abandon = () => { generation.current += 1; setBusy(false); };
  const lookup = async () => {
    const id = ++generation.current;
    const host = form.host.trim();
    const port = form.port;
    setBusy(true); setFailure(undefined);
    try {
      const key = await scanHostKey(host, port);
      if (id !== generation.current) return;
      setFetched(key); setAcknowledged(false);
      setForm((current) => ({ ...current, hostKey: `${key.algorithm} ${key.publicKey}` }));
    } catch (error) {
      if (id === generation.current) setFailure(safeErrorText(error, t("setupFetchFailed")));
    } finally {
      if (id === generation.current) setBusy(false);
    }
  };
  const editAddress = (patch: Partial<SshProfileRequest>) => {
    const hadFetched = fetched !== undefined;
    abandon(); setFetched(undefined); setFailure(undefined); setAcknowledged(false);
    setForm((current) => ({ ...current, ...patch, hostKey: hadFetched ? "" : current.hostKey }));
  };
  const editHostKey = (hostKey: string) => {
    setFetched(undefined); setAcknowledged(false);
    setForm((current) => ({ ...current, hostKey }));
  };
  const reset = () => { abandon(); setFetched(undefined); setFailure(undefined); };
  return { fetched, busy, failure, lookup, editAddress, editHostKey, reset };
}

export type HostKeyLookup = ReturnType<typeof useHostKeyLookup>;
