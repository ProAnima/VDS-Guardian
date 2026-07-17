import { useEffect, useState, type ReactNode } from "react";
import {
  Boxes, Check, CircleAlert, Container, Database, LoaderCircle, RefreshCw,
} from "lucide-react";
import type { Translate } from "../i18n";
import {
  hasTauriRuntime, listDockerContainers, type BackupSelectionItem, type DockerContainerSummary,
} from "../shared/commands";

interface DockerMountPickerProps {
  profileId: string;
  selectedItems: BackupSelectionItem[];
  onToggleItem: (item: BackupSelectionItem) => void;
  t: Translate;
}

interface CapturableMount {
  containerId: string; containerName: string; destination: string; kind: string; path: string; state: string;
}

interface CapturableGroup { groupId: string; paths: string[]; containers: string[]; running: boolean; }

export function DockerMountPicker({ profileId, selectedItems, onToggleItem, t }: DockerMountPickerProps) {
  const model = useDockerInventory(profileId, t);
  return <section className="docker-picker" aria-labelledby="docker-picker-title">
    <DockerHeader loading={model.loading} loaded={model.loaded} onLoad={model.load} profileId={profileId} t={t} />
    {model.failure && <div className="docker-picker__failure" role="alert"><CircleAlert size={17} /><div><strong>{t("dockerErrorTitle")}</strong><p>{model.failure}</p></div><button type="button" onClick={() => void model.load()}>{t("browserRetry")}</button></div>}
    {!model.loaded && !model.loading && !model.failure && <DockerWelcome t={t} />}
    {model.loading && !model.loaded && <DockerLoading t={t} />}
    {model.loaded && model.groups.length === 0 && model.mounts.length === 0 && <div className="docker-picker__empty"><Boxes size={24} /><strong>{t("dockerEmptyTitle")}</strong><p>{t("dockerEmpty")}</p></div>}
    {model.loaded && <DockerInventory groups={model.groups} mounts={model.mounts} onToggleItem={onToggleItem} selectedItems={selectedItems} t={t} />}
  </section>;
}

function DockerHeader({ loading, loaded, onLoad, profileId, t }: {
  loading: boolean; loaded: boolean; onLoad: () => Promise<void>; profileId: string; t: Translate;
}) {
  return <header className="docker-picker__header"><div className="docker-picker__title"><span><Container size={18} /></span><div><strong id="docker-picker-title">{t("dockerTitle")}</strong><p>{t("dockerBody")}</p></div></div><button aria-label={loaded ? t("dockerRefresh") : t("dockerShow")} className={loaded ? "docker-picker__refresh" : "button button--secondary"} disabled={!profileId || loading} title={loaded ? t("dockerRefresh") : undefined} type="button" onClick={() => void onLoad()}>{loading ? <LoaderCircle className="spin" size={16} /> : loaded ? <RefreshCw size={15} /> : <Container size={16} />}{!loaded && (loading ? t("dockerLoading") : t("dockerShow"))}</button></header>;
}

function DockerWelcome({ t }: { t: Translate }) {
  return <div className="docker-picker__welcome"><Boxes size={26} /><div><strong>{t("dockerReadyTitle")}</strong><p>{t("dockerReadyBody")}</p></div></div>;
}

function DockerLoading({ t }: { t: Translate }) {
  return <div className="docker-picker__loading" aria-live="polite"><LoaderCircle className="spin" size={19} /><span>{t("dockerLoading")}</span></div>;
}

function DockerInventory({ groups, mounts, selectedItems, onToggleItem, t }: {
  groups: CapturableGroup[]; mounts: CapturableMount[]; selectedItems: BackupSelectionItem[];
  onToggleItem: (item: BackupSelectionItem) => void; t: Translate;
}) {
  return <div className="docker-picker__inventory">
    {groups.length > 0 && <InventorySection icon={<Boxes size={15} />} title={t("dockerApplications")}><div className="docker-picker__cards">{groups.map((group) => <DockerGroupCard group={group} key={group.groupId} onToggleItem={onToggleItem} selectedItems={selectedItems} t={t} />)}</div></InventorySection>}
    {mounts.length > 0 && <InventorySection icon={<Database size={15} />} title={t("dockerStorage")}><div className="docker-picker__mounts">{mounts.map((mount) => <DockerMountRow key={`${mount.containerId}-${mount.destination}`} mount={mount} onToggleItem={onToggleItem} selectedItems={selectedItems} t={t} />)}</div></InventorySection>}
  </div>;
}

function InventorySection({ children, icon, title }: { children: ReactNode; icon: ReactNode; title: string }) {
  return <section className="docker-picker__section"><header>{icon}<strong>{title}</strong></header>{children}</section>;
}

function DockerGroupCard({ group, selectedItems, onToggleItem, t }: {
  group: CapturableGroup; selectedItems: BackupSelectionItem[]; onToggleItem: (item: BackupSelectionItem) => void; t: Translate;
}) {
  const item: BackupSelectionItem = { kind: "docker_group", groupId: group.groupId, capturablePaths: group.paths };
  const selected = isSelected(item, selectedItems);
  return <button aria-pressed={selected} className="docker-group-card" data-selected={selected || undefined} type="button" onClick={() => onToggleItem(item)}><span className="docker-group-card__icon"><Boxes size={19} /></span><span className="docker-group-card__copy"><strong>{group.groupId}</strong><small>{group.containers.length} {t("dockerContainersCount")} · {group.paths.length} {t("dockerPathsCount")}</small></span><span className="docker-group-card__state" data-running={group.running || undefined}>{group.running ? t("dockerActive") : t("dockerStopped")}</span><span className="docker-group-card__check">{selected && <Check size={14} />}</span></button>;
}

function DockerMountRow({ mount, selectedItems, onToggleItem, t }: {
  mount: CapturableMount; selectedItems: BackupSelectionItem[]; onToggleItem: (item: BackupSelectionItem) => void; t: Translate;
}) {
  const item: BackupSelectionItem = { kind: "docker_mount", containerId: mount.containerId, mountDestination: mount.destination, capturablePath: mount.path };
  const selected = isSelected(item, selectedItems);
  return <button aria-pressed={selected} className="docker-mount-row" data-selected={selected || undefined} type="button" onClick={() => onToggleItem(item)}><span className="docker-mount-row__check">{selected && <Check size={13} />}</span><Container size={17} /><span className="docker-mount-row__copy"><strong>{mount.containerName}</strong><code>{mount.path}</code><small>{mount.kind} → {mount.destination}</small></span><span className="docker-mount-row__state">{stateLabel(mount.state, t)}</span></button>;
}

function useDockerInventory(profileId: string, t: Translate) {
  const [mounts, setMounts] = useState<CapturableMount[]>([]);
  const [groups, setGroups] = useState<CapturableGroup[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [failure, setFailure] = useState<string>();
  useEffect(() => { setMounts([]); setGroups([]); setLoaded(false); setFailure(undefined); }, [profileId]);
  const load = async () => {
    if (!hasTauriRuntime() || !profileId) return;
    setLoading(true); setFailure(undefined);
    try { const containers = await listDockerContainers(profileId); setMounts(toMounts(containers)); setGroups(toGroups(containers)); setLoaded(true); }
    catch { setFailure(t("dockerError")); } finally { setLoading(false); }
  };
  return { groups, mounts, loaded, loading, failure, load };
}

function toMounts(containers: DockerContainerSummary[]): CapturableMount[] {
  return containers.flatMap((container) => container.mounts.filter((mount) => mount.capturablePath).map((mount) => ({
    containerId: container.id, containerName: container.name, destination: mount.destination,
    kind: mount.kind, path: mount.capturablePath as string, state: container.state,
  })));
}

function toGroups(containers: DockerContainerSummary[]): CapturableGroup[] {
  const groups = new Map<string, { paths: Set<string>; containers: Set<string>; running: boolean }>();
  for (const container of containers) {
    if (!container.composeProject) continue;
    const group = groups.get(container.composeProject) ?? { paths: new Set<string>(), containers: new Set<string>(), running: false };
    container.mounts.forEach((mount) => { if (mount.capturablePath) group.paths.add(mount.capturablePath); });
    group.containers.add(container.name); group.running ||= activeState(container.state);
    if (group.paths.size > 0) groups.set(container.composeProject, group);
  }
  return [...groups].map(([groupId, group]) => ({ groupId, paths: [...group.paths].sort(), containers: [...group.containers].sort(), running: group.running }));
}

function isSelected(item: BackupSelectionItem, selectedItems: BackupSelectionItem[]): boolean {
  return selectedItems.some((candidate) => JSON.stringify(candidate) === JSON.stringify(item));
}

function activeState(state: string): boolean { return ["running", "paused", "restarting"].includes(state); }
function stateLabel(state: string, t: Translate): string { return activeState(state) ? t("dockerActive") : t("dockerStopped"); }
