import type { CapturePlanSummary, RepositorySummary, SigningIdentityStatus, SshProfileSummary } from "../shared/commands";
import type { Translate } from "../i18n";

export type Readiness = "ready" | "attention";

export interface SetupResources {
  identity?: SigningIdentityStatus;
  repositories?: RepositorySummary[];
  profiles?: SshProfileSummary[];
  plans?: CapturePlanSummary[];
}

export interface SetupStatusItem { label: string; detail: string; readiness: Readiness; }

export function evaluateSetupReadiness(resources: SetupResources, t: Translate): SetupStatusItem[] {
  const repositories = resources.repositories;
  const readyRepositories = repositories?.filter((repository) => repository.recoveryReady).length ?? 0;
  return [
    identityItem(resources.identity, t), repositoryItem(repositories, readyRepositories, t), profileItem(resources.profiles, t), planItem(resources.plans, t),
  ];
}

function item(label: string, ready: boolean, detail: string): SetupStatusItem { return { label, detail, readiness: ready ? "ready" : "attention" }; }
function identityItem(identity: SigningIdentityStatus | undefined, t: Translate): SetupStatusItem { return item(t("readinessIdentity"), identity?.state === "ready", identity ? identity.state === "ready" ? t("readinessReady") : t("readinessCreateIdentity") : t("readinessCheckFailed")); }
function repositoryItem(repositories: RepositorySummary[] | undefined, ready: number, t: Translate): SetupStatusItem { return item(t("readinessRepository"), Boolean(repositories?.length) && ready === repositories?.length, repositories ? repositoryDetail(repositories.length, ready, t) : t("readinessCheckFailed")); }
function profileItem(profiles: SshProfileSummary[] | undefined, t: Translate): SetupStatusItem { return item(t("readinessServer"), Boolean(profiles?.length), profiles ? profiles.length > 0 ? `${t("readinessReady")}: ${profiles.length}` : t("readinessAddServer") : t("readinessCheckFailed")); }
function planItem(plans: CapturePlanSummary[] | undefined, t: Translate): SetupStatusItem { return item(t("readinessPlan"), Boolean(plans?.length), plans ? plans.length > 0 ? `${t("readinessReady")}: ${plans.length}` : t("readinessAddPlan") : t("readinessCheckFailed")); }
function repositoryDetail(total: number, ready: number, t: Translate): string { return total === 0 ? t("readinessAddRepository") : `${t("readinessRecoveryReady")} ${ready}/${total}.`; }
