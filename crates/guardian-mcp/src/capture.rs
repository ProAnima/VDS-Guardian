//! Capture tools: `plan_capture` previews an already-saved plan without
//! mutating anything; `run_capture` actually executes it. Capture has no
//! confirmation-phrase gate anywhere in this codebase today (desktop's
//! "Run" button is the confirmation) — `plan_capture` is a preview
//! convenience, not a hard precondition for `run_capture`, matching that
//! existing precedent rather than inventing a new gate only for MCP.

use crate::config::ServerConfig;
use crate::secret_store::resolve_store;
use guardian_capture::{
    CaptureInput, FilesystemCaptureComposition, SYSTEM_DISK_SPACE, build_capture_requests,
    current_timestamp, new_backup_id,
};
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::{
    BackupSelection, BackupSelectionItem, CancellationHandle, CaptureSelectionPreview,
    CaptureUseCaseError, DiscoverDockerInventoryUseCase, FilesystemCapturePlan, JobRegistry,
    PlanId, ProfileStorePort, RunId, preview_capture_selection,
};
use guardian_docker::SshDockerInventoryAdapter;
use guardian_local_repository::LocalRepository;
use guardian_profile_store::ProfileStore;
use guardian_signing::SigningIdentityManager;
use guardian_ssh::SystemOpenSsh;
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl CaptureFailure {
    fn plan() -> Self {
        Self {
            code: "capture_plan_not_ready",
            message: "The capture plan, server, or repository is unavailable.",
        }
    }
    fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
        }
    }
    fn repository() -> Self {
        Self {
            code: "repository_unavailable",
            message: "The backup repository could not be opened.",
        }
    }
    fn capture() -> Self {
        Self {
            code: "capture_failed",
            message: "The backup did not pass the verified capture lifecycle.",
        }
    }
    fn insufficient_space() -> Self {
        Self {
            code: "repository_disk_space_low",
            message: "The repository disk does not have enough free space for this backup; free space or use a larger disk. Nothing was written.",
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "capture_cancelled",
            message: "The capture was cancelled by the operator.",
        }
    }
    fn recovery_key_required() -> Self {
        Self {
            code: "recovery_key_not_configured",
            message: "This repository has no configured recovery key; run `recovery init` for it first.",
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The capture request could not be processed.",
        }
    }
    fn selection() -> Self {
        Self {
            code: "capture_selection_changed",
            message: "The selected server data changed or was not confirmed.",
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturePlanPreview {
    pub plan_id: String,
    pub profile_id: String,
    pub profile_label: String,
    pub repository_id: String,
    pub repository_label: String,
    pub roots: Vec<String>,
    pub database_path: Option<String>,
}

pub(crate) fn plan_capture(
    config: &ServerConfig,
    plan_id: &str,
) -> Result<CapturePlanPreview, CaptureFailure> {
    let stored = CapturePlanStore::at(&config.plans_dir)
        .list()
        .map_err(|_| CaptureFailure::plan())?
        .into_iter()
        .find(|stored| stored.plan.plan_id.as_str() == plan_id)
        .ok_or_else(CaptureFailure::plan)?;
    let profile = ProfileStore::at(&config.profiles_dir)
        .get(&stored.plan.profile_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    let registration = RepositoryStore::at(&config.repositories_dir)
        .get(&stored.plan.repository_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    Ok(CapturePlanPreview {
        plan_id: stored.plan.plan_id.as_str().to_owned(),
        profile_id: profile.profile_id.as_str().to_owned(),
        profile_label: profile.label,
        repository_id: registration.repository_id.as_str().to_owned(),
        repository_label: registration.label,
        roots: stored.plan.roots,
        database_path: stored.plan.database_path,
    })
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureJobSummary {
    pub backup_id: String,
}

pub(crate) fn run_capture(
    config: &ServerConfig,
    jobs: &Arc<JobRegistry>,
    plan_id: &str,
    run_id: &str,
) -> Result<CaptureJobSummary, CaptureFailure> {
    let run_id = RunId::parse(run_id).map_err(|_| CaptureFailure::internal())?;
    let handle = CancellationHandle::new();
    // Registered before the capture itself starts, so a concurrent
    // `cancel_job` tool call can find it while this run is still in flight.
    let _registration = jobs.register(run_id.clone(), handle.clone());
    let secrets = resolve_store(config.vault_dir.as_deref()).map_err(|_| CaptureFailure::plan())?;
    let stored = CapturePlanStore::at(&config.plans_dir)
        .list()
        .map_err(|_| CaptureFailure::plan())?
        .into_iter()
        .find(|stored| stored.plan.plan_id.as_str() == plan_id)
        .ok_or_else(CaptureFailure::plan)?;
    let profile = ProfileStore::at(&config.profiles_dir)
        .get(&stored.plan.profile_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    let registration = RepositoryStore::at(&config.repositories_dir)
        .get(&stored.plan.repository_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    let repository = LocalRepository::open(&registration.path, registration.repository_id)
        .map_err(|_| CaptureFailure::repository())?;
    let identity = SigningIdentityManager::open(&config.config_dir)
        .map_err(|_| CaptureFailure::signing())?
        .load_ready(&secrets)
        .map_err(|_| CaptureFailure::signing())?;
    let requests = build_capture_requests(CaptureInput {
        plan: &stored.plan,
        plan_sha256: &stored.sha256,
        source_layout: stored.source_layout,
        profile: &profile,
        run_id: &run_id,
        backup_id: new_backup_id().map_err(|_| CaptureFailure::internal())?,
        created_at: current_timestamp().map_err(|_| CaptureFailure::internal())?,
    })
    .map_err(|_| CaptureFailure::plan())?;
    let audit = NoopAudit;
    let ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let composition = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &ssh,
        profile: &profile,
        credentials: &secrets,
        audit: &audit,
        disk_space: &SYSTEM_DISK_SPACE,
        archive_limits: guardian_archive::ArchiveLimits::conservative(),
    };
    match composition.execute(requests.backup, requests.database, &identity) {
        Ok(sealed) => Ok(CaptureJobSummary {
            backup_id: sealed.backup_id.as_str().to_owned(),
        }),
        Err(CaptureUseCaseError::RecoveryKeyRequired) => {
            Err(CaptureFailure::recovery_key_required())
        }
        Err(CaptureUseCaseError::InsufficientRepositorySpace { .. }) => {
            Err(CaptureFailure::insufficient_space())
        }
        Err(_) if handle.is_cancelled() => Err(CaptureFailure::cancelled()),
        Err(_) => Err(CaptureFailure::capture()),
    }
}

pub(crate) fn preview_selection(
    config: &ServerConfig,
    selection: &BackupSelection,
) -> Result<CaptureSelectionPreview, CaptureFailure> {
    let profiles = ProfileStore::at(&config.profiles_dir);
    profiles
        .get(&selection.profile_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    RepositoryStore::at(&config.repositories_dir)
        .get(&selection.repository_id)
        .map_err(|_| CaptureFailure::plan())?
        .ok_or_else(CaptureFailure::plan)?;
    let inventory = selection_inventory(config, &profiles, selection)?;
    preview_capture_selection(selection, inventory.as_ref())
        .map_err(|_| CaptureFailure::selection())
}

pub(crate) fn execute_selection(
    config: &ServerConfig,
    jobs: &Arc<JobRegistry>,
    selection: &BackupSelection,
    confirmation: &str,
    run_id: &str,
) -> Result<CaptureJobSummary, CaptureFailure> {
    let preview = preview_selection(config, selection)?;
    if preview.confirmation != confirmation {
        return Err(CaptureFailure::selection());
    }
    let plan_id = save_selection_plan(config, &preview)?;
    run_capture(config, jobs, &plan_id, run_id)
}

fn selection_inventory(
    config: &ServerConfig,
    profiles: &ProfileStore,
    selection: &BackupSelection,
) -> Result<Option<guardian_core::DockerInventory>, CaptureFailure> {
    if !selection
        .items
        .iter()
        .any(|item| !matches!(item, BackupSelectionItem::RemotePath { .. }))
    {
        return Ok(None);
    }
    let secrets = resolve_store(config.vault_dir.as_deref()).map_err(|_| CaptureFailure::plan())?;
    let ssh = SystemOpenSsh::default();
    DiscoverDockerInventoryUseCase {
        profiles,
        inventory: &SshDockerInventoryAdapter {
            ssh: &ssh,
            credentials: &secrets,
        },
    }
    .execute(&selection.profile_id)
    .map(Some)
    .map_err(|_| CaptureFailure::selection())
}

fn save_selection_plan(
    config: &ServerConfig,
    preview: &CaptureSelectionPreview,
) -> Result<String, CaptureFailure> {
    let plan_id = PlanId::parse(random_id("plan")).map_err(|_| CaptureFailure::internal())?;
    let plan = FilesystemCapturePlan {
        plan_id: plan_id.clone(),
        version: 1,
        profile_id: preview.profile_id.clone(),
        repository_id: preview.repository_id.clone(),
        roots: preview
            .normalized_roots
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect(),
        database_path: preview
            .sqlite_path
            .as_ref()
            .map(|path| path.as_str().to_owned()),
    };
    let stored = guardian_configuration::StoredCapturePlan::new(plan)
        .and_then(|stored| stored.with_source_layout(preview.source_layout.clone()))
        .map_err(|_| CaptureFailure::selection())?;
    CapturePlanStore::at(&config.plans_dir)
        .upsert(stored)
        .map_err(|_| CaptureFailure::plan())?;
    Ok(plan_id.as_str().to_owned())
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "{prefix}-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

struct NoopAudit;
impl guardian_core::AuditPort for NoopAudit {
    fn capture_failed(&self, _: &RunId, _: guardian_core::CaptureAuditCode) {}
}
