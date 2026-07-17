//! Capture tools: `plan_capture` previews an already-saved plan without
//! mutating anything; `run_capture` actually executes it. Capture has no
//! confirmation-phrase gate anywhere in this codebase today (desktop's
//! "Run" button is the confirmation) — `plan_capture` is a preview
//! convenience, not a hard precondition for `run_capture`, matching that
//! existing precedent rather than inventing a new gate only for MCP.

use crate::config::ServerConfig;
use crate::secret_store::resolve_store;
use guardian_capture::{FilesystemCaptureComposition, SYSTEM_DISK_SPACE};
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::{
    BackupId, BackupSelection, BackupSelectionItem, CancellationHandle, CaptureSelectionPreview,
    CaptureUseCaseError, DiscoverDockerInventoryUseCase, EmbeddedDatabaseCaptureRequest,
    FilesystemBackupRequest, FilesystemCapturePlan, FilesystemCaptureRequest, JobRegistry,
    Manifest, PayloadPath, PlanId, PlanReference, Producer, ProfileStorePort, RunId,
    SourceIdentity, Timestamp, preview_capture_selection,
};
use guardian_docker::SshDockerInventoryAdapter;
use guardian_local_repository::LocalRepository;
use guardian_profile_store::ProfileStore;
use guardian_signing::SigningIdentityManager;
use guardian_ssh::SystemOpenSsh;
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let backup_id = BackupId::parse(random_id("backup")).map_err(|_| CaptureFailure::internal())?;
    let created_at = now_timestamp()?;
    let manifest = Manifest::new(
        backup_id.clone(),
        run_id.clone(),
        created_at.clone(),
        Producer {
            name: "VDS Guardian".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: std::env::consts::OS.to_owned(),
        },
        SourceIdentity {
            profile_id: profile.profile_id.clone(),
            host_key_fingerprint: guardian_core::host_key_fingerprint(
                &profile.endpoint.host_pin.public_key_base64,
            ),
        },
        PlanReference {
            plan_id: stored.plan.plan_id.clone(),
            version: stored.plan.version,
            sha256: stored.sha256,
        },
    );
    let database_path = stored.plan.database_path;
    let request = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: stored.plan.roots,
            payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst.enc")
                .map_err(|_| CaptureFailure::internal())?,
        },
        manifest,
        sealed_at: created_at,
    };
    let database = match database_path {
        Some(database_path) => Some(EmbeddedDatabaseCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            database_path,
            payload_path: PayloadPath::parse("payload/database-000.sqlite.zst.enc")
                .map_err(|_| CaptureFailure::internal())?,
        }),
        None => None,
    };
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
    match composition.execute(request, database, &identity) {
        Ok(sealed) => Ok(CaptureJobSummary {
            backup_id: sealed.backup_id.as_str().to_owned(),
        }),
        Err(CaptureUseCaseError::RecoveryKeyRequired) => {
            Err(CaptureFailure::recovery_key_required())
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

fn now_timestamp() -> Result<Timestamp, CaptureFailure> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CaptureFailure::internal())?
        .as_secs();
    let days = i64::try_from(seconds / 86_400).map_err(|_| CaptureFailure::internal())?;
    let (year, month, day) = civil_date(days);
    let day_seconds = seconds % 86_400;
    Timestamp::parse(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        (day_seconds / 60) % 60,
        day_seconds % 60
    ))
    .map_err(|_| CaptureFailure::internal())
}

fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    (
        y + i64::from(mp >= 10),
        u32::try_from(mp + if mp < 10 { 3 } else { -9 }).unwrap_or(1),
        u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1),
    )
}

struct NoopAudit;
impl guardian_core::AuditPort for NoopAudit {
    fn capture_failed(&self, _: &RunId, _: guardian_core::CaptureAuditCode) {}
}
