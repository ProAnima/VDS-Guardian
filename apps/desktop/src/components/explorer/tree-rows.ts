import type { RemoteBrowseEntry } from "../../shared/commands";
import type { DockerContainerNode, DockerMountNode, DockerProjectNode } from "./docker-model";
import type { DockerTree } from "./useDockerTree";
import type { FileTree } from "./useFileTree";

export type SectionId = "docker" | "files";
export type NoteKind = "empty" | "failure" | "loading";

export type TreeRow =
  | { key: string; type: "section"; section: SectionId; open: boolean; loading: boolean; failed: boolean; count?: number }
  | { key: string; type: "entry"; depth: number; entry: RemoteBrowseEntry; open: boolean; loading: boolean }
  | { key: string; type: "more"; depth: number; path: string; loading: boolean }
  | { key: string; type: "note"; depth: number; note: NoteKind; text?: string; path?: string }
  | { key: string; type: "project"; depth: number; project: DockerProjectNode; open: boolean }
  | { key: string; type: "container"; depth: number; container: DockerContainerNode; open: boolean }
  | { key: string; type: "mount"; depth: number; mount: DockerMountNode };

export type SectionState = Record<SectionId, boolean>;

export function buildRows(files: FileTree, docker: DockerTree, sections: SectionState): TreeRow[] {
  return [...dockerRows(docker, sections.docker), ...fileRows(files, sections.files)];
}

function dockerRows(docker: DockerTree, open: boolean): TreeRow[] {
  const { model, status } = docker;
  const count = model.projects.length + model.standalone.length;
  const head: TreeRow = { key: "section:docker", type: "section", section: "docker", open, loading: status === "loading", failed: status === "unavailable", count: status === "ready" ? count : undefined };
  if (!open || status !== "ready") return [head];
  if (count === 0) return [head, { key: "docker:empty", type: "note", depth: 1, note: "empty" }];
  return [head, ...model.projects.flatMap((project) => projectRows(project, docker)), ...model.standalone.flatMap((container) => containerRows(container, 1, docker))];
}

function projectRows(project: DockerProjectNode, docker: DockerTree): TreeRow[] {
  const open = docker.expanded.has(`project:${project.id}`);
  const row: TreeRow = { key: `project:${project.id}`, type: "project", depth: 1, project, open };
  return open ? [row, ...project.containers.flatMap((container) => containerRows(container, 2, docker))] : [row];
}

function containerRows(container: DockerContainerNode, depth: number, docker: DockerTree): TreeRow[] {
  const open = docker.expanded.has(`container:${container.id}`);
  const row: TreeRow = { key: `container:${container.id}`, type: "container", depth, container, open };
  if (!open) return [row];
  return [row, ...container.mounts.map((mount): TreeRow => ({ key: `mount:${mount.id}`, type: "mount", depth: depth + 1, mount }))];
}

function fileRows(files: FileTree, open: boolean): TreeRow[] {
  const root = files.directories["/"];
  const head: TreeRow = { key: "section:files", type: "section", section: "files", open, loading: Boolean(root?.loading), failed: Boolean(root?.failure && root.entries.length === 0) };
  return open ? [head, ...directoryRows("/", 1, files)] : [head];
}

function directoryRows(path: string, depth: number, files: FileTree): TreeRow[] {
  const state = files.directories[path];
  if (!state) return [{ key: `loading:${path}`, type: "note", depth, note: "loading" }];
  const rows: TreeRow[] = state.entries.flatMap((entry) => entryRows(entry, depth, files));
  if (state.failure) rows.push({ key: `failure:${path}`, type: "note", depth, note: "failure", text: state.failure, path });
  else if (state.loading && state.entries.length === 0) rows.push({ key: `loading:${path}`, type: "note", depth, note: "loading" });
  else if (state.entries.length === 0) rows.push({ key: `empty:${path}`, type: "note", depth, note: "empty" });
  if (state.truncated && state.nextCursor) rows.push({ key: `more:${path}`, type: "more", depth, path, loading: state.loading });
  return rows;
}

function entryRows(entry: RemoteBrowseEntry, depth: number, files: FileTree): TreeRow[] {
  const open = entry.kind === "directory" && files.expanded.has(entry.absolutePath);
  const row: TreeRow = { key: `entry:${entry.absolutePath}`, type: "entry", depth, entry, open, loading: Boolean(files.directories[entry.absolutePath]?.loading) };
  return open ? [row, ...directoryRows(entry.absolutePath, depth + 1, files)] : [row];
}
