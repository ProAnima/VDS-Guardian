import type { BackupSelectionItem, DockerContainerSummary } from "../../shared/commands";

export type MountReason = "not_persistent" | "unresolved";

export interface DockerMountNode {
  id: string; containerId: string; containerName: string; kind: "bind" | "volume" | "tmpfs";
  destination: string; path?: string; reason?: MountReason;
}
export interface DockerContainerNode { id: string; name: string; state: DockerContainerSummary["state"]; active: boolean; mounts: DockerMountNode[]; }
export interface DockerProjectNode { id: string; containers: DockerContainerNode[]; paths: string[]; active: boolean; }
export interface DockerModel { projects: DockerProjectNode[]; standalone: DockerContainerNode[]; }

const activeStates = new Set(["running", "paused", "restarting"]);
export const isActive = (state: string): boolean => activeStates.has(state);

export function buildDockerModel(containers: DockerContainerSummary[]): DockerModel {
  const projects = new Map<string, DockerContainerNode[]>();
  const standalone: DockerContainerNode[] = [];
  for (const container of containers) {
    const node = toContainer(container);
    if (!container.composeProject) { if (hasPersistent(node)) standalone.push(node); continue; }
    projects.set(container.composeProject, [...(projects.get(container.composeProject) ?? []), node]);
  }
  return {
    projects: [...projects].flatMap(([id, nodes]) => toProject(id, nodes)).sort((a, b) => a.id.localeCompare(b.id)),
    standalone: standalone.sort((a, b) => a.name.localeCompare(b.name)),
  };
}

function toProject(id: string, containers: DockerContainerNode[]): DockerProjectNode[] {
  const paths = [...new Set(containers.flatMap((container) => container.mounts.flatMap((mount) => (mount.path ? [mount.path] : []))))].sort();
  if (paths.length === 0) return [];
  return [{ id, containers: containers.sort((a, b) => a.name.localeCompare(b.name)), paths, active: containers.some((container) => container.active) }];
}

function toContainer(container: DockerContainerSummary): DockerContainerNode {
  return {
    id: container.id, name: container.name, state: container.state, active: isActive(container.state),
    mounts: container.mounts.map((mount) => ({
      id: `${container.id}:${mount.destination}`, containerId: container.id, containerName: container.name,
      kind: mount.kind, destination: mount.destination, path: mount.capturablePath,
      reason: mount.capturablePath ? undefined : mount.kind === "tmpfs" ? "not_persistent" : "unresolved",
    })),
  };
}

export const hasPersistent = (container: DockerContainerNode): boolean => container.mounts.some((mount) => mount.path);

export function mountItem(mount: DockerMountNode): BackupSelectionItem {
  return { kind: "docker_mount", containerId: mount.containerId, mountDestination: mount.destination, capturablePath: mount.path ?? "" };
}

export function groupItem(project: DockerProjectNode): BackupSelectionItem {
  return { kind: "docker_group", groupId: project.id, capturablePaths: project.paths };
}

export function selectableMounts(container: DockerContainerNode): DockerMountNode[] {
  return container.mounts.filter((mount) => mount.path);
}
