import type { CapturePlanSummary, RepositorySummary, SigningIdentityStatus, SshProfileSummary } from "../shared/commands";

export type Readiness = "ready" | "attention";

export interface SetupResources {
  identity?: SigningIdentityStatus;
  repositories?: RepositorySummary[];
  profiles?: SshProfileSummary[];
  plans?: CapturePlanSummary[];
}

export interface SetupStatusItem { label: string; detail: string; readiness: Readiness; }

export function evaluateSetupReadiness(resources: SetupResources): SetupStatusItem[] {
  const repositories = resources.repositories;
  const readyRepositories = repositories?.filter((repository) => repository.recoveryReady).length ?? 0;
  return [
    identityItem(resources.identity), repositoryItem(repositories, readyRepositories), profileItem(resources.profiles), planItem(resources.plans),
  ];
}

function item(label: string, ready: boolean, detail: string): SetupStatusItem { return { label, detail, readiness: ready ? "ready" : "attention" }; }
function identityItem(identity: SigningIdentityStatus | undefined): SetupStatusItem { return item("Подписывающая идентичность", identity?.state === "ready", identity ? identity.state === "ready" ? "Готово" : "Создайте или восстановите идентичность." : "Не удалось проверить. Обновите статус."); }
function repositoryItem(repositories: RepositorySummary[] | undefined, ready: number): SetupStatusItem { return item("Хранилище и recovery", Boolean(repositories?.length) && ready === repositories?.length, repositories ? repositoryDetail(repositories.length, ready) : "Не удалось проверить. Обновите статус."); }
function profileItem(profiles: SshProfileSummary[] | undefined): SetupStatusItem { return item("SSH-сервер", Boolean(profiles?.length), profiles ? profiles.length > 0 ? `Готово: ${profiles.length}` : "Добавьте и проверьте SSH-сервер." : "Не удалось проверить. Обновите статус."); }
function planItem(plans: CapturePlanSummary[] | undefined): SetupStatusItem { return item("План бэкапа", Boolean(plans?.length), plans ? plans.length > 0 ? `Готово: ${plans.length}` : "Выберите сервер, хранилище и пути для сохранения." : "Не удалось проверить. Обновите статус."); }
function repositoryDetail(total: number, ready: number): string { return total === 0 ? "Создайте хранилище и подготовьте recovery-ключ." : `Recovery готово: ${ready}/${total}.`; }
