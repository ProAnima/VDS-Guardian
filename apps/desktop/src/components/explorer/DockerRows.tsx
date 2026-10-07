import { Ban, Boxes, Container, FolderInput, HardDrive } from "lucide-react";
import type { Translate } from "../../i18n";
import type { BackupSelectionItem } from "../../shared/commands";
import {
  groupItem, mountItem, selectableMounts, type DockerContainerNode, type DockerMountNode, type DockerProjectNode,
} from "./docker-model";
import { RowFrame } from "./RowFrame";
import { hasItem, pathCoverage } from "./selection";
import type { TreeRow } from "./tree-rows";

export interface DockerRowContext {
  t: Translate;
  items: BackupSelectionItem[];
  onSetDocker: (items: BackupSelectionItem[], selected: boolean) => void;
  onToggleNode: (id: string) => void;
}

type Of<K extends TreeRow["type"]> = Extract<TreeRow, { type: K }>;

export function ProjectRow({ row, context }: { row: Of<"project">; context: DockerRowContext }) {
  const { project, depth, open } = row;
  const { t, items } = context;
  const group = groupItem(project);
  const checked = hasItem(items, group);
  const partial = !checked && project.containers.some((container) => selectableMounts(container).some((mount) => pathCoverage(items, mount.path ?? "") !== undefined));
  return (
    <RowFrame
      depth={depth} icon={Boxes} tone="docker" name={project.id} open={open} openLabel={t(open ? "explorerCollapse" : "explorerExpand")}
      onToggleOpen={() => context.onToggleNode(row.key)} state={project.active ? "active" : "stopped"} selected={checked}
      badge={String(project.containers.length)} hint={`${project.containers.length} ${t("dockerContainersCount")} · ${project.paths.length} ${t("dockerPathsCount")}`}
      check={{ checked, indeterminate: partial, disabled: false, label: `${t("browserSelect")} ${project.id}`, onChange: () => context.onSetDocker([group], !checked) }}
    />
  );
}

export function ContainerRow({ row, context }: { row: Of<"container">; context: DockerRowContext }) {
  const { container, depth, open } = row;
  const { t, items } = context;
  const mounts = selectableMounts(container);
  const states = mounts.map((mount) => pathCoverage(items, mount.path ?? ""));
  const all = mounts.length > 0 && states.every((state) => state !== undefined);
  const coveredOnly = all && states.every((state) => state === "covered");
  return (
    <RowFrame
      depth={depth} icon={Container} tone="docker" name={container.name} open={open} openLabel={t(open ? "explorerCollapse" : "explorerExpand")}
      onToggleOpen={() => context.onToggleNode(row.key)} state={container.active ? "active" : "stopped"} dimmed={mounts.length === 0}
      selected={all && !coveredOnly} hint={mounts.length === 0 ? t("dockerNotPersistent") : coveredOnly ? t("browserCoveredReason") : undefined}
      check={{
        checked: all, indeterminate: !all && states.some((state) => state !== undefined), disabled: mounts.length === 0 || coveredOnly,
        label: `${t("browserSelect")} ${container.name}`, onChange: () => context.onSetDocker(mounts.map(mountItem), !all),
      }}
    />
  );
}

export function MountRow({ row, context }: { row: Of<"mount">; context: DockerRowContext }) {
  const { mount, depth } = row;
  const { t, items } = context;
  const coverage = mount.path ? pathCoverage(items, mount.path) : undefined;
  return (
    <RowFrame
      depth={depth} icon={mountIcon(mount)} tone="docker" name={mount.destination} detail={mount.path} dimmed={!mount.path}
      selected={coverage === "selected"} hint={mountHint(mount, coverage, t)} size={mount.kind}
      check={{
        checked: coverage !== undefined, disabled: !mount.path || coverage === "covered",
        label: `${t("browserSelect")} ${mount.destination}`, onChange: () => context.onSetDocker([mountItem(mount)], coverage === undefined),
      }}
    />
  );
}

function mountIcon(mount: DockerMountNode) {
  if (!mount.path) return Ban;
  return mount.kind === "volume" ? HardDrive : FolderInput;
}

function mountHint(mount: DockerMountNode, coverage: ReturnType<typeof pathCoverage>, t: Translate): string | undefined {
  if (mount.reason === "not_persistent") return t("dockerNotPersistent");
  if (mount.reason === "unresolved") return t("dockerUnresolved");
  return coverage === "covered" ? t("browserCoveredReason") : mount.path;
}

export type { DockerContainerNode, DockerProjectNode };
