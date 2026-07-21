import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowUp, ChevronRight, CircleAlert, File, Folder, FolderCheck, HardDrive,
  Link, LoaderCircle, RefreshCw, Server,
} from "lucide-react";
import type { Translate } from "../i18n";
import {
  browseRemoteDirectory, hasTauriRuntime, type RemoteBrowseEntry, type RemoteBrowsePage,
} from "../shared/commands";
import { safeErrorText } from "../shared/safe-error";

interface RemoteFileExplorerProps {
  profileId: string;
  selectedPaths: string[];
  onTogglePath: (path: string) => void;
  t: Translate;
}

export function RemoteFileExplorer(props: RemoteFileExplorerProps) {
  const model = useRemoteBrowser(props.profileId, props.t);
  return (
    <section className="remote-browser" aria-labelledby="remote-browser-title">
      <ExplorerHeader model={model} onTogglePath={props.onTogglePath} selectedPaths={props.selectedPaths} t={props.t} />
      {!model.page && <ExplorerWelcome model={model} profileId={props.profileId} t={props.t} />}
      {model.failure && <ExplorerFailure message={model.failure} onRetry={() => void model.open(model.directory)} t={props.t} />}
      {model.page && <ExplorerTable model={model} selectedPaths={props.selectedPaths} onTogglePath={props.onTogglePath} t={props.t} />}
    </section>
  );
}

function ExplorerHeader({ model, selectedPaths, onTogglePath, t }: {
  model: RemoteBrowserModel; selectedPaths: string[]; onTogglePath: (path: string) => void; t: Translate;
}) {
  const currentSelected = selectedPaths.includes(model.directory);
  const coveredBy = coveringAncestor(selectedPaths, model.directory);
  return (
    <header className="remote-browser__header">
      <div className="remote-browser__title">
        <span><HardDrive size={18} /></span>
        <strong id="remote-browser-title">{t("browserTitle")}</strong>
      </div>
      {model.page && <button className="button button--secondary remote-browser__select-current" disabled={Boolean(coveredBy)} title={coveredBy ? `${t("browserCoveredReason")} ${coveredBy}` : undefined} type="button" onClick={() => onTogglePath(model.directory)}>
        <FolderCheck size={16} />{coveredBy ? t("browserFolderCovered") : currentSelected ? t("browserDeselectFolder") : t("browserSelectFolder")}
      </button>}
    </header>
  );
}

function ExplorerWelcome({ model, profileId, t }: { model: RemoteBrowserModel; profileId: string; t: Translate }) {
  const desktop = hasTauriRuntime();
  return (
    <div className="remote-browser__welcome">
      <span className="remote-browser__welcome-icon"><Server size={28} /></span>
      <strong>{profileId ? t("browserReadyTitle") : t("browserChooseServer")}</strong>
      <p>{profileId ? t("browserReadyBody") : t("browserChooseServerBody")}</p>
      <button className="button button--primary" disabled={!profileId || model.loading || !desktop} type="button" onClick={() => void model.open("/")}>
        {model.loading ? <LoaderCircle className="spin" size={16} /> : <HardDrive size={16} />}
        {model.loading ? t("browserLoading") : t("browserOpen")}
      </button>
      {!desktop && <small>{t("browserDesktop")}</small>}
    </div>
  );
}

function ExplorerFailure({ message, onRetry, t }: { message: string; onRetry: () => void; t: Translate }) {
  return <div className="remote-browser__failure" role="alert">
    <CircleAlert size={18} /><div><strong>{t("browserFailureTitle")}</strong><p>{message}</p></div>
    <button type="button" onClick={onRetry}><RefreshCw size={14} />{t("browserRetry")}</button>
  </div>;
}

function ExplorerTable({ model, selectedPaths, onTogglePath, t }: {
  model: RemoteBrowserModel; selectedPaths: string[]; onTogglePath: (path: string) => void; t: Translate;
}) {
  return <div className="remote-browser__workspace" aria-busy={model.loading}>
    <BrowserToolbar model={model} t={t} />
    <div className="remote-browser__table" role="table" aria-label={t("browserContents")}>
      <div className="remote-browser__columns" role="row"><span /><span>{t("browserName")}</span><span>{t("browserModified")}</span><span>{t("browserSize")}</span></div>
      <BrowserEntries entries={model.page?.entries ?? []} selectedPaths={selectedPaths} onOpen={model.open} onTogglePath={onTogglePath} t={t} />
    </div>
    {model.page?.entries.length === 0 && <div className="remote-browser__empty"><Folder size={24} /><span>{t("browserEmpty")}</span></div>}
    {model.page?.truncated && <button className="button button--secondary remote-browser__more" disabled={model.loading} type="button" onClick={() => void model.more()}>{model.loading && <LoaderCircle className="spin" size={16} />}{t("browserMore")}</button>}
    {model.loading && !model.page && <div className="remote-browser__loading"><LoaderCircle className="spin" size={18} />{t("browserLoading")}</div>}
  </div>;
}

function BrowserToolbar({ model, t }: { model: RemoteBrowserModel; t: Translate }) {
  return <div className="remote-browser__toolbar">
    <button aria-label={t("browserUp")} disabled={model.loading || model.directory === "/"} title={t("browserUp")} type="button" onClick={() => void model.open(parentDirectory(model.directory))}><ArrowUp size={15} /></button>
    <nav aria-label={t("browserLocation")}><Breadcrumbs directory={model.directory} onOpen={model.open} /></nav>
    <button aria-label={t("browserRefresh")} disabled={model.loading} title={t("browserRefresh")} type="button" onClick={() => void model.open(model.directory)}><RefreshCw className={model.loading ? "spin" : undefined} size={15} /></button>
  </div>;
}

function Breadcrumbs({ directory, onOpen }: { directory: string; onOpen: (path: string) => Promise<void> }) {
  const parts = directory.split("/").filter(Boolean);
  return <><button type="button" onClick={() => void onOpen("/")}><HardDrive size={14} /><span>/</span></button>{parts.map((part, index) => {
    const path = `/${parts.slice(0, index + 1).join("/")}`;
    return <span className="remote-browser__crumb" key={path}><ChevronRight size={13} /><button aria-current={path === directory ? "page" : undefined} type="button" onClick={() => void onOpen(path)}>{part}</button></span>;
  })}</>;
}

function BrowserEntries({ entries, selectedPaths, onOpen, onTogglePath, t }: {
  entries: RemoteBrowseEntry[]; selectedPaths: string[]; onOpen: (path: string) => Promise<void>;
  onTogglePath: (path: string) => void; t: Translate;
}) {
  return <div className="remote-browser__entries" role="rowgroup">{entries.map((entry) => <BrowserEntry coveredBy={coveringAncestor(selectedPaths, entry.absolutePath)} entry={entry} key={entry.absolutePath} onOpen={onOpen} onTogglePath={onTogglePath} selected={selectedPaths.includes(entry.absolutePath)} t={t} />)}</div>;
}

function BrowserEntry({ coveredBy, entry, selected, onOpen, onTogglePath, t }: {
  coveredBy?: string; entry: RemoteBrowseEntry; selected: boolean; onOpen: (path: string) => Promise<void>;
  onTogglePath: (path: string) => void; t: Translate;
}) {
  const coveredReason = coveredBy ? `${t("browserCoveredReason")} ${coveredBy}` : undefined;
  const reason = coveredReason ?? unavailableLabel(entry, t);
  const unavailable = !entry.selectable || Boolean(coveredBy);
  return <div className="remote-browser__entry" data-covered={coveredBy || undefined} data-disabled={unavailable || undefined} data-selected={selected || undefined} role="row" title={reason}>
    <input aria-label={`${t("browserSelect")} ${entry.name}`} checked={selected || Boolean(coveredBy)} disabled={unavailable} type="checkbox" onChange={() => onTogglePath(entry.absolutePath)} />
    <div className="remote-browser__name" role="cell"><EntryIcon kind={entry.kind} />{entry.kind === "directory" ? <button type="button" onClick={() => void onOpen(entry.absolutePath)}>{entry.name}</button> : <span>{entry.name}</span>}{reason && <small>{reason}</small>}</div>
    <time role="cell">{formatModified(entry.modifiedAt, t)}</time><span role="cell">{entry.kind === "regular_file" ? formatSize(entry.size ?? 0) : "—"}</span>
  </div>;
}

function EntryIcon({ kind }: { kind: RemoteBrowseEntry["kind"] }) {
  if (kind === "directory") return <Folder size={18} />;
  if (kind === "regular_file") return <File size={18} />;
  return <Link size={18} />;
}

function useRemoteBrowser(profileId: string, t: Translate) {
  const [directory, setDirectory] = useState("/");
  const [page, setPage] = useState<RemoteBrowsePage>();
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string>();
  const requestId = useRef(0);
  const open = useCallback(async (path: string) => {
    if (!profileId || !hasTauriRuntime()) return;
    const request = ++requestId.current;
    setLoading(true); setFailure(undefined);
    try {
      const next = await readDirectory(profileId, path);
      if (request === requestId.current) { setDirectory(next.directory); setPage(next); }
    } catch (error) {
      if (request === requestId.current) setFailure(safeErrorText(error, t("browserFailure")));
    } finally { if (request === requestId.current) setLoading(false); }
  }, [profileId, t]);
  useEffect(() => {
    requestId.current += 1; setDirectory("/"); setPage(undefined); setFailure(undefined);
    if (profileId && hasTauriRuntime()) void open("/");
  }, [open, profileId]);
  const more = async () => {
    if (!page?.nextCursor || loading) return;
    const request = ++requestId.current;
    setLoading(true); setFailure(undefined);
    try { const next = await browseRemoteDirectory(profileId, directory, page.nextCursor); if (request === requestId.current) setPage({ ...next, entries: [...page.entries, ...next.entries] }); }
    catch (error) { if (request === requestId.current) setFailure(safeErrorText(error, t("browserFailure"))); }
    finally { if (request === requestId.current) setLoading(false); }
  };
  return { directory, page, loading, failure, open, more };
}

const pendingDirectories = new Map<string, Promise<RemoteBrowsePage>>();

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

type RemoteBrowserModel = ReturnType<typeof useRemoteBrowser>;

function unavailableLabel(entry: RemoteBrowseEntry, t: Translate): string | undefined {
  if (entry.unavailableReason === "symlink") return t("browserSymlinkReason");
  if (entry.unavailableReason === "special_file") return t("browserSpecialReason");
  return undefined;
}

function formatModified(value: string | undefined, t: Translate): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? t("browserUnknownDate") : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"]; let value = bytes / 1024; let unit = units[0];
  for (let index = 1; value >= 1024 && index < units.length; index += 1) { value /= 1024; unit = units[index]; }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

function coveringAncestor(paths: string[], path: string): string | undefined {
  return paths.find((selected) => selected !== path && (selected === "/" || path.startsWith(`${selected}/`)));
}

function parentDirectory(path: string): string {
  const parts = path.split("/").filter(Boolean); parts.pop();
  return parts.length === 0 ? "/" : `/${parts.join("/")}`;
}
