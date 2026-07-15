import { useState } from "react";
import { CircleAlert, Container, LoaderCircle } from "lucide-react";
import { hasTauriRuntime, listDockerContainers, type DockerContainerSummary } from "../shared/commands";

interface DockerMountPickerProps {
  profileId: string;
  onAddPath: (path: string) => void;
}

interface CapturableMount {
  containerId: string;
  containerName: string;
  destination: string;
  kind: string;
  path: string;
}

export function DockerMountPicker({ profileId, onAddPath }: DockerMountPickerProps) {
  const model = useCapturableMounts(profileId);
  return (
    <div className="repository-form__actions">
      <button
        className="button button--secondary"
        disabled={!profileId || model.loading}
        type="button"
        onClick={() => void model.load()}
      >
        {model.loading ? <LoaderCircle className="spin" size={16} /> : <Container size={16} />}
        {model.loading ? "Ищем контейнеры…" : "Показать Docker-контейнеры"}
      </button>
      {model.failure && (
        <p className="signing-panel__error" role="alert">
          <CircleAlert size={16} />
          {model.failure}
        </p>
      )}
      {model.loaded && model.mounts.length === 0 && (
        <p className="restore-panel__empty">Нет контейнеров с доступными для захвата путями.</p>
      )}
      <CapturableMountList mounts={model.mounts} onAddPath={onAddPath} />
    </div>
  );
}

function CapturableMountList({ mounts, onAddPath }: { mounts: CapturableMount[]; onAddPath: (path: string) => void }) {
  if (mounts.length === 0) return null;
  return (
    <div className="repository-panel__items">
      {mounts.map((mount) => (
        <button
          key={`${mount.containerId}-${mount.destination}`}
          type="button"
          title={`${mount.kind} → ${mount.destination}`}
          onClick={() => onAddPath(mount.path)}
        >
          {mount.containerName} · {mount.path}
        </button>
      ))}
    </div>
  );
}

function toCapturableMounts(containers: DockerContainerSummary[]): CapturableMount[] {
  return containers.flatMap((container) =>
    container.mounts
      .filter((mount) => mount.capturablePath)
      .map((mount) => ({
        containerId: container.id,
        containerName: container.name,
        destination: mount.destination,
        kind: mount.kind,
        path: mount.capturablePath as string,
      })),
  );
}

function useCapturableMounts(profileId: string) {
  const [mounts, setMounts] = useState<CapturableMount[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string>();

  const load = async () => {
    if (!hasTauriRuntime() || !profileId) return;
    setLoading(true);
    setFailure(undefined);
    try {
      setMounts(toCapturableMounts(await listDockerContainers(profileId)));
      setLoaded(true);
    } catch {
      setFailure("Не удалось получить список Docker-контейнеров с этого сервера.");
    } finally {
      setLoading(false);
    }
  };

  return { mounts, loaded, loading, failure, load };
}
