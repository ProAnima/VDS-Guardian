use guardian_capture::{
    CaptureInput, FilesystemCaptureComposition, SYSTEM_DISK_SPACE, build_capture_requests,
    current_timestamp, new_backup_id,
};
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::{
    BackupSelection, CancellationHandle, CaptureUseCaseError, JobRegistry, ProfileStorePort, RunId,
};
use guardian_local_repository::LocalRepository;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_signing::SigningIdentityManager;
use guardian_ssh::SystemOpenSsh;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, path::PathBuf};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCapturePlanRequest {
    pub(crate) plan_id: String,
    pub(crate) run_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCaptureSelectionRequest {
    selection: BackupSelection,
    confirmation: String,
    run_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureJobSummary {
    pub(crate) backup_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureJobFailure {
    pub code: &'static str,
    pub message: Cow<'static, str>,
    pub remediation: Cow<'static, str>,
}

pub async fn run(
    app: tauri::AppHandle,
    request: RunCapturePlanRequest,
) -> Result<CaptureJobSummary, CaptureJobFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| CaptureJobFailure::storage())?;
    let run_id = RunId::parse(&request.run_id).map_err(|_| CaptureJobFailure::internal())?;
    let handle = CancellationHandle::new();
    // Registered here, before `spawn_blocking`, specifically so a
    // concurrent `cancel_job` call fired while this job is still running
    // can find it -- registering inside `run_blocking` itself would be too
    // late for that race.
    let registry = app.state::<JobRegistry>();
    let _registration = registry.register(run_id.clone(), handle.clone());
    tauri::async_runtime::spawn_blocking(move || run_blocking(root, request, run_id, handle))
        .await
        .map_err(|_| CaptureJobFailure::internal())?
}

pub async fn run_selection(
    app: tauri::AppHandle,
    request: RunCaptureSelectionRequest,
) -> Result<CaptureJobSummary, CaptureJobFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| CaptureJobFailure::storage())?;
    let run_id = RunId::parse(&request.run_id).map_err(|_| CaptureJobFailure::internal())?;
    let handle = CancellationHandle::new();
    let registry = app.state::<JobRegistry>();
    let _registration = registry.register(run_id.clone(), handle.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let plan = crate::plan_commands::save_confirmed_selection_blocking(
            root.clone(),
            request.selection,
            &request.confirmation,
        )
        .map_err(|failure| match failure.code {
            "capture_selection_confirmation_required" | "invalid_capture_plan" => {
                CaptureJobFailure::selection()
            }
            "plan_storage_unavailable" => CaptureJobFailure::storage(),
            _ => CaptureJobFailure::plan(),
        })?;
        run_blocking(
            root,
            RunCapturePlanRequest {
                plan_id: plan.plan_id,
                run_id: request.run_id,
            },
            run_id,
            handle,
        )
    })
    .await
    .map_err(|_| CaptureJobFailure::internal())?
}

pub(crate) fn run_blocking(
    root: PathBuf,
    request: RunCapturePlanRequest,
    run_id: RunId,
    handle: CancellationHandle,
) -> Result<CaptureJobSummary, CaptureJobFailure> {
    let plan = CapturePlanStore::at(root.join("plans"))
        .list()
        .map_err(|_| CaptureJobFailure::storage())?
        .into_iter()
        .find(|stored| stored.plan.plan_id.as_str() == request.plan_id)
        .ok_or_else(CaptureJobFailure::plan)?;
    let profile = ProfileStore::at(root.join("profiles"))
        .get(&plan.plan.profile_id)
        .map_err(|_| CaptureJobFailure::storage())?
        .ok_or_else(CaptureJobFailure::plan)?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&plan.plan.repository_id)
        .map_err(|_| CaptureJobFailure::storage())?
        .ok_or_else(CaptureJobFailure::plan)?;
    let repository = LocalRepository::open(&registration.path, registration.repository_id)
        .map_err(|_| CaptureJobFailure::repository())?;
    let identity = SigningIdentityManager::open(root.join("node"))
        .map_err(|_| CaptureJobFailure::signing())?
        .load_ready(&OsCredentialStore)
        .map_err(|_| CaptureJobFailure::signing())?;
    let requests = build_capture_requests(CaptureInput {
        plan: &plan.plan,
        plan_sha256: &plan.sha256,
        source_layout: plan.source_layout,
        profile: &profile,
        run_id: &run_id,
        backup_id: new_backup_id().map_err(|_| CaptureJobFailure::internal())?,
        created_at: current_timestamp().map_err(|_| CaptureJobFailure::internal())?,
    })
    .map_err(|_| CaptureJobFailure::plan())?;
    let audit = NoopAudit;
    let ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let composition = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &ssh,
        profile: &profile,
        credentials: &OsCredentialStore,
        audit: &audit,
        disk_space: &SYSTEM_DISK_SPACE,
        archive_limits: guardian_archive::ArchiveLimits::conservative(),
    };
    let sealed = match composition.execute(requests.backup, requests.database, &identity) {
        Ok(sealed) => sealed,
        Err(CaptureUseCaseError::RecoveryKeyRequired) => {
            return Err(CaptureJobFailure::recovery_key_required());
        }
        Err(CaptureUseCaseError::InsufficientRepositorySpace {
            available_bytes,
            required_bytes,
        }) => {
            return Err(CaptureJobFailure::insufficient_space(
                available_bytes,
                required_bytes,
            ));
        }
        Err(_) if handle.is_cancelled() => return Err(CaptureJobFailure::cancelled()),
        Err(_) => return Err(CaptureJobFailure::capture()),
    };
    Ok(CaptureJobSummary {
        backup_id: sealed.backup_id.as_str().to_owned(),
    })
}

/// One-decimal GiB for operator-facing disk messages.
fn format_gib(bytes: u64) -> String {
    const GIB: f64 = 1_073_741_824.0;
    #[allow(clippy::cast_precision_loss)]
    let gib = bytes as f64 / GIB;
    format!("{gib:.1} GiB")
}

struct NoopAudit;
impl guardian_core::AuditPort for NoopAudit {
    fn capture_failed(&self, _: &RunId, _: guardian_core::CaptureAuditCode) {}
}

impl CaptureJobFailure {
    fn plan() -> Self {
        Self {
            code: "capture_plan_not_ready",
            message: "The capture plan, server, or repository is unavailable.".into(),
            remediation: "Refresh setup data and complete all setup steps.".into(),
        }
    }
    fn selection() -> Self {
        Self {
            code: "capture_selection_changed",
            message: "The selected server data changed or was not confirmed.".into(),
            remediation: "Review the selection again before creating the backup.".into(),
        }
    }
    fn signing() -> Self {
        Self {
            code: "signing_identity_not_ready",
            message: "The backup signing identity is not ready.".into(),
            remediation: "Complete signing identity setup before starting a backup.".into(),
        }
    }
    fn repository() -> Self {
        Self {
            code: "repository_unavailable",
            message: "The backup repository could not be opened.".into(),
            remediation: "Reconnect or repair the selected backup location.".into(),
        }
    }
    fn capture() -> Self {
        Self {
            code: "capture_failed",
            message: "The backup did not pass the verified capture lifecycle.".into(),
            remediation: "Review the pinned SSH preflight, free-space reserve, and server access."
                .into(),
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "capture_cancelled",
            message: "The backup was cancelled by the operator.".into(),
            remediation: "Start a new backup run if it should still happen.".into(),
        }
    }
    fn recovery_key_required() -> Self {
        Self {
            code: "recovery_key_not_configured",
            message: "This repository has no configured recovery key.".into(),
            remediation: "Run `guardian-cli recovery init` for this repository before capturing."
                .into(),
        }
    }
    fn insufficient_space(available_bytes: u64, required_bytes: u64) -> Self {
        Self {
            code: "repository_disk_space_low",
            message: format!(
                "The backup disk has {} free but this backup needs at least {}.",
                format_gib(available_bytes),
                format_gib(required_bytes)
            )
            .into(),
            remediation: "Free space on the backup disk or register a repository on a larger \
                          disk, then start the backup again. Nothing was written."
                .into(),
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.".into(),
            remediation: "Check local storage and try again.".into(),
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The desktop command did not complete.".into(),
            remediation: "Try again and export redacted diagnostics if the problem persists."
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureJobFailure, format_gib};

    #[test]
    fn low_disk_failure_names_both_sizes_and_says_nothing_was_written() {
        let failure = CaptureJobFailure::insufficient_space(13_600_000_000, 48_318_382_080);
        assert_eq!(failure.code, "repository_disk_space_low");
        assert!(failure.message.contains("12.7 GiB"));
        assert!(failure.message.contains("45.0 GiB"));
        assert!(failure.remediation.contains("Nothing was written"));
    }

    #[test]
    fn gib_formatting_is_one_decimal() {
        assert_eq!(format_gib(0), "0.0 GiB");
        assert_eq!(format_gib(1_073_741_824), "1.0 GiB");
    }
}
