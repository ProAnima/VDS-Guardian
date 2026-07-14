use guardian_capture::FilesystemCaptureComposition;
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::{
    BackupId, FilesystemBackupRequest, FilesystemCaptureRequest, Manifest, PayloadPath,
    PlanReference, Producer, ProfileStorePort, RunId, SourceIdentity, Timestamp,
};
use guardian_local_repository::LocalRepository;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_signing::SigningIdentityManager;
use guardian_ssh::SystemOpenSsh;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCapturePlanRequest {
    plan_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureJobSummary {
    pub backup_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureJobFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn run(
    app: tauri::AppHandle,
    request: RunCapturePlanRequest,
) -> Result<CaptureJobSummary, CaptureJobFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| CaptureJobFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || run_blocking(root, request))
        .await
        .map_err(|_| CaptureJobFailure::internal())?
}

fn run_blocking(
    root: PathBuf,
    request: RunCapturePlanRequest,
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
    let run_id = RunId::parse(random_id("run")).map_err(|_| CaptureJobFailure::internal())?;
    let backup_id =
        BackupId::parse(random_id("backup")).map_err(|_| CaptureJobFailure::internal())?;
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
            host_key_fingerprint: fingerprint(&profile.endpoint.host_pin.public_key_base64),
        },
        PlanReference {
            plan_id: plan.plan.plan_id.clone(),
            version: plan.plan.version,
            sha256: plan.sha256,
        },
    );
    let request = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: plan.plan.roots,
            payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst")
                .map_err(|_| CaptureJobFailure::internal())?,
        },
        manifest,
        sealed_at: created_at,
    };
    repository
        .write_capture_audit(&run_id, "started", None)
        .map_err(|_| CaptureJobFailure::repository())?;
    let audit = NoopAudit;
    let composition = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &SystemOpenSsh::default(),
        profile: &profile,
        credentials: &OsCredentialStore,
        audit: &audit,
        archive_limits: guardian_archive::ArchiveLimits::conservative(),
    };
    let sealed = match composition.execute(request, &identity) {
        Ok(sealed) => sealed,
        Err(_) => {
            let _ = repository.write_capture_audit(&run_id, "failed", None);
            return Err(CaptureJobFailure::capture());
        }
    };
    repository
        .write_capture_audit(&run_id, "sealed", Some(&sealed.backup_id))
        .map_err(|_| CaptureJobFailure::repository())?;
    Ok(CaptureJobSummary {
        backup_id: sealed.backup_id.as_str().to_owned(),
    })
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
fn fingerprint(key: &str) -> String {
    format!("SHA256:{:x}", Sha256::digest(key.as_bytes()))
}

fn now_timestamp() -> Result<Timestamp, CaptureJobFailure> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CaptureJobFailure::internal())?
        .as_secs();
    let days = i64::try_from(seconds / 86_400).map_err(|_| CaptureJobFailure::internal())?;
    let (year, month, day) = civil_date(days);
    let day_seconds = seconds % 86_400;
    Timestamp::parse(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        (day_seconds / 60) % 60,
        day_seconds % 60
    ))
    .map_err(|_| CaptureJobFailure::internal())
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

impl CaptureJobFailure {
    fn plan() -> Self {
        Self {
            code: "capture_plan_not_ready",
            message: "The capture plan, server, or repository is unavailable.",
            remediation: "Refresh setup data and complete all setup steps.",
        }
    }
    fn signing() -> Self {
        Self {
            code: "signing_identity_not_ready",
            message: "The backup signing identity is not ready.",
            remediation: "Complete signing identity setup before starting a backup.",
        }
    }
    fn repository() -> Self {
        Self {
            code: "repository_unavailable",
            message: "The backup repository could not be opened.",
            remediation: "Reconnect or repair the selected backup location.",
        }
    }
    fn capture() -> Self {
        Self {
            code: "capture_failed",
            message: "The backup did not pass the verified capture lifecycle.",
            remediation: "Review the pinned SSH preflight, free-space reserve, and server access.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check local storage and try again.",
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The desktop command did not complete.",
            remediation: "Try again and export redacted diagnostics if the problem persists.",
        }
    }
}
