import { useCallback, useEffect, useRef, useState } from "react";
import { hasTauriRuntime, listDockerContainers } from "../../shared/commands";
import { buildDockerModel, type DockerModel } from "./docker-model";

export type DockerStatus = "idle" | "loading" | "ready" | "unavailable";

const emptyModel: DockerModel = { projects: [], standalone: [] };

/** Reads the Docker inventory as soon as a server is chosen, so containers appear next to the file tree. */
export function useDockerTree(profileId: string) {
  const [status, setStatus] = useState<DockerStatus>("idle");
  const [model, setModel] = useState<DockerModel>(emptyModel);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const epoch = useRef(0);
  const load = useCallback(async () => {
    if (!profileId || !hasTauriRuntime()) return;
    const current = epoch.current;
    setStatus("loading");
    try {
      const containers = await listDockerContainers(profileId);
      if (current !== epoch.current) return;
      setModel(buildDockerModel(containers)); setStatus("ready");
    } catch {
      if (current === epoch.current) setStatus("unavailable");
    }
  }, [profileId]);
  useEffect(() => {
    epoch.current += 1;
    setModel(emptyModel); setExpanded(new Set()); setStatus("idle");
    void load();
  }, [load]);
  const toggle = (id: string) => setExpanded((current) => { const next = new Set(current); if (!next.delete(id)) next.add(id); return next; });
  return { status, model, expanded, toggle, reload: load };
}

export type DockerTree = ReturnType<typeof useDockerTree>;
