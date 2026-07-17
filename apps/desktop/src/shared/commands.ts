import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export interface FoundationStatus {
  product: string;
  version: string;
  iteration: string;
  liveOperationsEnabled: boolean;
}

export type SigningIdentityState =
  | "not_enrolled"
  | "enrollment_pending"
  | "recovery_pending"
  | "ready";

export type EnrollmentDisposition = "enrolled" | "recovered" | "loaded";

export interface SigningIdentityDescriptor {
  credentialId: string;
  algorithm: string;
  keyId: string;
}

export interface SigningIdentityStatus {
  state: SigningIdentityState;
  identity: SigningIdentityDescriptor | null;
}

export interface SigningIdentityEnrollment {
  disposition: EnrollmentDisposition;
  identity: SigningIdentityDescriptor;
}

export interface SigningIdentityFailure {
  code: string;
  message: string;
  remediation: string;
}

export interface SshProfileRequest {
  label: string;
  host: string;
  port: number;
  user: string;
  hostKey: string;
  keyPath: string;
}

export interface SshProfileSummary {
  profileId: string;
  label: string;
  host: string;
  port: number;
  user: string;
}

export interface SshProfileFailure {
  code: string;
  message: string;
  remediation: string;
}

export interface SshPreflightSummary {
  tarZstd: boolean;
}

export interface RepositoryRequest {
  label: string;
  path: string;
}

export interface RepositorySummary {
  repositoryId: string;
  label: string;
  path: string;
  recoveryReady: boolean;
}

export interface RepositoryFailure {
  code: string;
  message: string;
  remediation: string;
}
export interface ExportRecoveryBundleRequest {
  repositoryId: string;
  passphrase: string;
  passphraseConfirmation: string;
  outputPath: string;
  confirmation: string;
}
export interface ImportRecoveryBundleRequest {
  repositoryId: string;
  repositoryPath: string;
  inputPath: string;
  passphrase: string;
  confirmation: string;
}
export interface CapturePlanRequest { profileId: string; repositoryId: string; roots: string[]; databasePath?: string; }
export interface CapturePlanSummary { planId: string; profileId: string; repositoryId: string; roots: string[]; databasePath?: string; }
export type BackupSelectionItem =
  | { kind: "remote_path"; absolutePath: string }
  | { kind: "docker_mount"; containerId: string; mountDestination: string; capturablePath: string }
  | { kind: "docker_group"; groupId: string; capturablePaths: string[] };
export interface BackupSelection { profileId: string; repositoryId: string; items: BackupSelectionItem[]; sqlitePath?: string; }
export type CaptureSelectionWarning =
  | { kind: "covered_path"; path: string; coveredBy: string }
  | { kind: "live_docker_data"; containerId: string; containerName: string }
  | { kind: "sqlite_also_in_filesystem"; sqlitePath: string; coveredBy: string };
export interface CaptureSelectionPreview { profileId: string; repositoryId: string; normalizedRoots: string[]; logicalItems: BackupSelectionItem[]; warnings: CaptureSelectionWarning[]; sqlitePath?: string; confirmation: string; }
export interface CaptureJobSummary { backupId: string; }
export interface CaptureSelectionExecutionRequest { selection: BackupSelection; confirmation: string; runId: string; }
export interface CaptureFailure { code: string; message: string; remediation: string; }

export interface BackupSummary { backupId: string; sealedAt: string; verification: "verified"; }
export interface RestoreRequest { repositoryId: string; backupId: string; destination: string; confirmation?: string; runId?: string; }
export interface RestorePreview { backupId: string; destination: string; confirmation: string; payload: string; }
export interface RestoreFailure { code: string; message: string; remediation: string; }

export interface DeployRequest { repositoryId: string; backupId: string; targetProfileId: string; targetPath: string; confirmation?: string; runId?: string; }
export interface DeploymentPreview { backupId: string; targetProfileId: string; targetProfileLabel: string; targetPath: string; confirmation: string; filesystemPayload: string; databasePayload?: string; }
export interface DeployFailure { code: string; message: string; remediation: string; }

export interface DockerMountSummary { kind: "bind" | "volume" | "tmpfs"; destination: string; capturablePath?: string; }
export interface DockerContainerSummary { id: string; name: string; composeProject?: string; state: "created" | "running" | "paused" | "restarting" | "exited" | "dead"; mounts: DockerMountSummary[]; }
export interface DockerCommandFailure { code: string; message: string; remediation: string; }
export type RemoteEntryKind = "directory" | "regular_file" | "symlink" | "other";
export interface RemoteBrowseEntry { name: string; absolutePath: string; kind: RemoteEntryKind; size?: number; modifiedAt?: string; selectable: boolean; unavailableReason?: "symlink" | "special_file"; }
export interface RemoteBrowsePage { directory: string; entries: RemoteBrowseEntry[]; nextCursor?: string; truncated: boolean; }

export const previewStatus: FoundationStatus = {
  product: "VDS Guardian",
  version: "0.1.0",
  iteration: "Release 0.1 validation — operator path in progress",
  liveOperationsEnabled: true,
};

export async function getFoundationStatus(): Promise<FoundationStatus> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return previewStatus;
  }

  return invoke<FoundationStatus>("get_foundation_status");
}

export async function getSigningIdentityStatus(): Promise<SigningIdentityStatus> {
  if (!hasTauriRuntime()) {
    return { state: "not_enrolled", identity: null };
  }

  return invoke<SigningIdentityStatus>("get_signing_identity_status");
}

export async function enrollSigningIdentity(): Promise<SigningIdentityEnrollment> {
  if (!hasTauriRuntime()) {
    throw new Error("Signing enrollment requires the VDS Guardian desktop runtime.");
  }

  return invoke<SigningIdentityEnrollment>("enroll_signing_identity");
}

export async function enrollSshProfile(request: SshProfileRequest): Promise<SshProfileSummary> {
  requireTauriRuntime();
  return invoke<SshProfileSummary>("enroll_ssh_profile", { request });
}

export async function listSshProfiles(): Promise<SshProfileSummary[]> {
  if (!hasTauriRuntime()) return [];
  return invoke<SshProfileSummary[]>("list_ssh_profiles");
}

export async function deleteSshProfile(profileId: string): Promise<void> {
  requireTauriRuntime();
  return invoke<void>("delete_ssh_profile", { request: { profileId, confirmed: true } });
}

export async function testSshProfile(profileId: string): Promise<void> {
  requireTauriRuntime();
  return invoke<void>("test_ssh_profile", { profileId });
}

export async function preflightSshProfile(profileId: string): Promise<SshPreflightSummary> {
  requireTauriRuntime();
  return invoke<SshPreflightSummary>("preflight_ssh_profile", { profileId });
}

export async function registerRepository(request: RepositoryRequest): Promise<RepositorySummary> {
  requireTauriRuntime();
  return invoke<RepositorySummary>("register_repository", { request });
}

export async function listRepositories(): Promise<RepositorySummary[]> {
  if (!hasTauriRuntime()) return [];
  return invoke<RepositorySummary[]>("list_repositories");
}
export async function initializeRepositoryRecovery(repositoryId: string): Promise<void> {
  requireTauriRuntime();
  return invoke<void>("initialize_repository_recovery", { repositoryId });
}
export async function exportRecoveryBundle(request: ExportRecoveryBundleRequest): Promise<void> {
  requireTauriRuntime();
  return invoke<void>("export_recovery_bundle", { request });
}
export async function importRecoveryBundle(request: ImportRecoveryBundleRequest): Promise<RepositorySummary> {
  requireTauriRuntime();
  return invoke<RepositorySummary>("import_recovery_bundle", { request });
}
export async function saveCapturePlan(request: CapturePlanRequest): Promise<CapturePlanSummary> { requireTauriRuntime(); return invoke<CapturePlanSummary>("save_capture_plan", { request }); }
export async function previewCaptureSelection(request: BackupSelection): Promise<CaptureSelectionPreview> { requireTauriRuntime(); return invoke<CaptureSelectionPreview>("preview_capture_selection", { request }); }
export async function listCapturePlans(): Promise<CapturePlanSummary[]> { if (!hasTauriRuntime()) return []; return invoke<CapturePlanSummary[]>("list_capture_plans"); }
export async function runCapturePlan(planId: string, runId: string): Promise<CaptureJobSummary> { requireTauriRuntime(); return invoke<CaptureJobSummary>("run_capture_plan", { request: { planId, runId } }); }
export async function runCaptureSelection(request: CaptureSelectionExecutionRequest): Promise<CaptureJobSummary> { requireTauriRuntime(); return invoke<CaptureJobSummary>("run_capture_selection", { request }); }
export async function cancelJob(runId: string): Promise<boolean> { if (!hasTauriRuntime()) return false; return invoke<boolean>("cancel_job", { runId }); }
export async function listBackups(repositoryId: string): Promise<BackupSummary[]> { if (!hasTauriRuntime()) return []; return invoke<BackupSummary[]>("list_backups", { repositoryId }); }
export async function previewRestore(request: RestoreRequest): Promise<RestorePreview> { requireTauriRuntime(); return invoke<RestorePreview>("preview_restore", { request }); }
export async function executeRestore(request: RestoreRequest): Promise<RestorePreview> { requireTauriRuntime(); return invoke<RestorePreview>("execute_restore", { request }); }
export async function previewDeploy(request: DeployRequest): Promise<DeploymentPreview> { requireTauriRuntime(); return invoke<DeploymentPreview>("preview_deploy", { request }); }
export async function executeDeploy(request: DeployRequest): Promise<DeploymentPreview> { requireTauriRuntime(); return invoke<DeploymentPreview>("execute_deploy", { request }); }
export async function listDockerContainers(profileId: string): Promise<DockerContainerSummary[]> { requireTauriRuntime(); return invoke<DockerContainerSummary[]>("list_docker_containers", { profileId }); }
export async function browseRemoteDirectory(profileId: string, directory: string, cursor?: string): Promise<RemoteBrowsePage> { requireTauriRuntime(); return invoke<RemoteBrowsePage>("browse_remote_directory", { request: { profileId, directory, cursor, limit: 100 } }); }
// OpenSSH commonly stores private identities without a conventional extension
// (for example `id_rsa` or an operator-provided `id_rsa.priv`). File type is
// validated by the desktop command after selection, never by this UI filter.
export async function pickSshKeyPath(): Promise<string | undefined> { return pickPath({ directory: false }); }
export async function pickRepositoryPath(): Promise<string | undefined> { return pickPath({ directory: true }); }
export async function pickRecoveryBundlePath(): Promise<string | undefined> {
  if (!hasTauriRuntime()) return undefined;
  const selected = await save({ defaultPath: "guardian-recovery-bundle.json" });
  return typeof selected === "string" ? selected : undefined;
}
export async function pickRecoveryBundleInput(): Promise<string | undefined> { return pickPath({ directory: false }); }

export function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function requireTauriRuntime(): void {
  if (!hasTauriRuntime()) throw new Error("SSH profile enrollment requires the VDS Guardian desktop runtime.");
}
async function pickPath(options: Parameters<typeof open>[0]): Promise<string | undefined> { if (!hasTauriRuntime()) return undefined; const selected = await open(options); return typeof selected === "string" ? selected : undefined; }
