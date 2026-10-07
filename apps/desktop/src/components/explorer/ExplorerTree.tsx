import { useLayoutEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type RefObject } from "react";
import { CircleAlert, Container, HardDrive, RefreshCw } from "lucide-react";
import type { Translate } from "../../i18n";
import type { BackupSelectionItem } from "../../shared/commands";
import { tip } from "../../shared/tip";
import { ContainerRow, MountRow, ProjectRow } from "./DockerRows";
import { EntryRow, MoreRow, NoteRow } from "./FileRows";
import { RowFrame } from "./RowFrame";
import { remotePaths } from "./selection";
import { buildRows, type SectionId, type SectionState, type TreeRow } from "./tree-rows";
import { useDockerTree } from "./useDockerTree";
import { useFileTree } from "./useFileTree";

interface ExplorerTreeProps {
  profileId: string;
  items: BackupSelectionItem[];
  onTogglePath: (path: string) => void;
  onSetDocker: (items: BackupSelectionItem[], selected: boolean) => void;
  t: Translate;
}

export function ExplorerTree({ profileId, items, onTogglePath, onSetDocker, t }: ExplorerTreeProps) {
  const files = useFileTree(profileId, t);
  const docker = useDockerTree(profileId);
  const [sections, setSections] = useState<SectionState>({ docker: true, files: true });
  const treeRef = useRef<HTMLDivElement>(null);
  const trackFocus = useRovingTabStop(treeRef);
  const rows = buildRows(files, docker, sections);
  const fileContext = { t, paths: remotePaths(items), onTogglePath, onToggleDirectory: files.toggle, onMore: files.more, onReload: files.reload };
  const dockerContext = { t, items, onSetDocker, onToggleNode: docker.toggle };
  const sectionActions = {
    docker: { reload: () => void docker.reload(), failure: t("dockerErrorTitle"), refresh: t("dockerRefresh") },
    files: { reload: () => files.reload("/"), failure: t("browserFailure"), refresh: t("browserRefresh") },
  };
  return (
    <div className="explorer-tree" ref={treeRef} role="tree" aria-multiselectable="true" aria-label={t("backupChooseDataTitle")} onFocus={trackFocus} onKeyDown={(event) => moveFocus(event, treeRef.current)}>
      {rows.map((row) => {
        if (row.type === "section") return <SectionRow key={row.key} row={row} t={t} actions={sectionActions[row.section]} onToggle={() => setSections((current) => ({ ...current, [row.section]: !current[row.section] }))} />;
        if (row.type === "entry") return <EntryRow key={row.key} row={row} context={fileContext} />;
        if (row.type === "more") return <MoreRow key={row.key} row={row} context={fileContext} />;
        if (row.type === "note") return <NoteRow key={row.key} row={row} context={fileContext} />;
        if (row.type === "project") return <ProjectRow key={row.key} row={row} context={dockerContext} />;
        if (row.type === "container") return <ContainerRow key={row.key} row={row} context={dockerContext} />;
        return <MountRow key={row.key} row={row} context={dockerContext} />;
      })}
    </div>
  );
}

interface SectionActions { reload: () => void; failure: string; refresh: string }

function SectionRow({ row, t, actions, onToggle }: {
  row: Extract<TreeRow, { type: "section" }>; t: Translate; actions: SectionActions; onToggle: () => void;
}) {
  const docker = row.section === "docker";
  const label = t(docker ? "explorerDocker" : "explorerFiles");
  return (
    <RowFrame
      depth={0} icon={docker ? Container : HardDrive} tone="section" name={label} open={row.open} loading={row.loading}
      openLabel={t(row.open ? "explorerCollapse" : "explorerExpand")} onToggleOpen={onToggle} onReload={actions.reload}
      badge={row.count === undefined ? undefined : String(row.count)} hint={row.failed ? actions.failure : undefined}
    >
      {row.failed && <CircleAlert className="tree-row__alert" size={14} aria-hidden="true" />}
      <button className="tree-row__action" type="button" tabIndex={-1} disabled={row.loading} onClick={actions.reload} {...tip(actions.refresh)}>
        <RefreshCw className={row.loading ? "spin" : undefined} size={13} aria-hidden="true" />
      </button>
    </RowFrame>
  );
}

/**
 * Roving tab stop: exactly one row is in the Tab order (the last one focused, or the first row),
 * so Tab enters and leaves the tree in one step and the arrow keys move within it.
 */
function useRovingTabStop(tree: RefObject<HTMLDivElement | null>) {
  const active = useRef<HTMLElement | null>(null);
  const apply = (current: HTMLElement | null) => {
    const rows = [...(tree.current?.querySelectorAll<HTMLElement>("[data-row]") ?? [])];
    const stop = current && rows.includes(current) ? current : rows[0];
    for (const row of rows) row.tabIndex = row === stop ? 0 : -1;
  };
  useLayoutEffect(() => apply(active.current));
  return (event: FocusEvent<HTMLDivElement>) => {
    const row = (event.target as HTMLElement).closest<HTMLElement>("[data-row]");
    if (row) { active.current = row; apply(row); }
  };
}

function moveFocus(event: KeyboardEvent<HTMLDivElement>, tree: HTMLDivElement | null) {
  if (!tree || (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "Home" && event.key !== "End")) return;
  const rows = [...tree.querySelectorAll<HTMLElement>("[data-row]")];
  const current = rows.findIndex((row) => row === document.activeElement || row.contains(document.activeElement));
  const last = rows.length - 1;
  const next = event.key === "Home" ? 0 : event.key === "End" ? last : Math.min(last, Math.max(0, current + (event.key === "ArrowDown" ? 1 : -1)));
  event.preventDefault();
  rows[next]?.focus();
}

export type { SectionId };
