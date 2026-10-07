import { useCallback, useEffect, useRef, useState } from "react";
import type { Translate } from "../../i18n";
import { browseRemoteDirectory, hasTauriRuntime, type RemoteBrowseEntry, type RemoteBrowsePage } from "../../shared/commands";
import { safeErrorText } from "../../shared/safe-error";

export interface DirectoryState {
  entries: RemoteBrowseEntry[]; nextCursor?: string; truncated: boolean; loading: boolean; failure?: string;
}

const pendingDirectories = new Map<string, Promise<RemoteBrowsePage>>();

/** Shares one in-flight request per directory so a StrictMode remount never reads the server twice. */
function readDirectory(profileId: string, path: string): Promise<RemoteBrowsePage> {
  const key = `${profileId}\n${path}`;
  const pending = pendingDirectories.get(key);
  if (pending) return pending;
  const request = browseRemoteDirectory(profileId, path);
  pendingDirectories.set(key, request);
  const clear = () => { if (pendingDirectories.get(key) === request) pendingDirectories.delete(key); };
  void request.then(clear, clear);
  return request;
}

const emptyDirectory: DirectoryState = { entries: [], truncated: false, loading: false };

export function useFileTree(profileId: string, t: Translate) {
  const [directories, setDirectories] = useState<Record<string, DirectoryState>>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set(["/"]));
  const epoch = useRef(0);
  const patch = useCallback((path: string, change: (current: DirectoryState) => DirectoryState) => {
    setDirectories((all) => ({ ...all, [path]: change(all[path] ?? emptyDirectory) }));
  }, []);
  const load = useCallback(async (path: string, cursor?: string) => {
    if (!profileId || !hasTauriRuntime()) return;
    const current = epoch.current;
    patch(path, (state) => ({ ...state, loading: true, failure: undefined }));
    try {
      const page = cursor ? await browseRemoteDirectory(profileId, path, cursor) : await readDirectory(profileId, path);
      if (current !== epoch.current) return;
      patch(path, (state) => ({
        entries: cursor ? [...state.entries, ...page.entries] : page.entries,
        nextCursor: page.nextCursor, truncated: page.truncated, loading: false,
      }));
    } catch (error) {
      if (current === epoch.current) patch(path, (state) => ({ ...state, loading: false, failure: safeErrorText(error, t("browserFailure")) }));
    }
  }, [patch, profileId, t]);
  useEffect(() => {
    epoch.current += 1;
    setDirectories({}); setExpanded(new Set(["/"]));
    if (profileId && hasTauriRuntime()) void load("/");
  }, [load, profileId]);
  const toggle = (path: string) => {
    const next = new Set(expanded);
    if (next.delete(path)) { setExpanded(next); return; }
    next.add(path); setExpanded(next);
    if (!directories[path]) void load(path);
  };
  const more = (path: string) => { const cursor = directories[path]?.nextCursor; if (cursor && !directories[path]?.loading) void load(path, cursor); };
  const reload = (path: string) => void load(path);
  return { directories, expanded, toggle, more, reload };
}

export type FileTree = ReturnType<typeof useFileTree>;
