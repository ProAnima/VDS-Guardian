//! One place that turns a saved capture plan into the requests the capture
//! use case executes. Desktop and `guardian-mcp` both call this instead of
//! assembling the manifest themselves, so the same plan yields the same
//! request through either surface by construction.

use guardian_core::{
    BackupId, EmbeddedDatabaseCaptureRequest, FilesystemBackupRequest, FilesystemCapturePlan,
    FilesystemCaptureRequest, Manifest, PayloadPath, PlanReference, Producer, RunId,
    SourceIdentity, SourceLayout, Timestamp, VdsProfile, host_key_fingerprint,
};
use rand_core::{OsRng, RngCore};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const FILESYSTEM_PAYLOAD_PATH: &str = "payload/filesystem-000.tar.zst.enc";
pub const DATABASE_PAYLOAD_PATH: &str = "payload/database-000.sqlite.zst.enc";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureBuildError {
    #[error("the saved plan and server profile do not describe the same capture")]
    Invalid,
    #[error("a backup identifier or timestamp could not be generated")]
    Unavailable,
}

pub struct CaptureInput<'a> {
    pub plan: &'a FilesystemCapturePlan,
    pub plan_sha256: &'a str,
    pub source_layout: Option<SourceLayout>,
    pub profile: &'a VdsProfile,
    pub run_id: &'a RunId,
    pub backup_id: BackupId,
    pub created_at: Timestamp,
}

pub struct CaptureRequests {
    pub backup: FilesystemBackupRequest,
    pub database: Option<EmbeddedDatabaseCaptureRequest>,
}

/// Pure: no clock and no randomness, so it is deterministic and testable.
pub fn build_capture_requests(
    input: CaptureInput<'_>,
) -> Result<CaptureRequests, CaptureBuildError> {
    let CaptureInput {
        plan,
        plan_sha256,
        source_layout,
        profile,
        run_id,
        backup_id,
        created_at,
    } = input;
    if plan.profile_id != profile.profile_id {
        return Err(CaptureBuildError::Invalid);
    }
    let mut manifest = Manifest::new(
        backup_id,
        run_id.clone(),
        created_at.clone(),
        Producer {
            name: "VDS Guardian".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: std::env::consts::OS.to_owned(),
        },
        SourceIdentity {
            profile_id: profile.profile_id.clone(),
            host_key_fingerprint: host_key_fingerprint(
                &profile.endpoint.host_pin.public_key_base64,
            ),
        },
        PlanReference {
            plan_id: plan.plan_id.clone(),
            version: plan.version,
            sha256: plan_sha256.to_owned(),
        },
    );
    manifest.source_layout = source_layout;
    let payload = |path: &str| PayloadPath::parse(path).map_err(|_| CaptureBuildError::Invalid);
    let backup = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: plan.roots.clone(),
            payload_path: payload(FILESYSTEM_PAYLOAD_PATH)?,
        },
        manifest,
        sealed_at: created_at,
    };
    let database = match &plan.database_path {
        Some(database_path) => Some(EmbeddedDatabaseCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            database_path: database_path.clone(),
            payload_path: payload(DATABASE_PAYLOAD_PATH)?,
        }),
        None => None,
    };
    Ok(CaptureRequests { backup, database })
}

/// `backup-` followed by 128 random bits in hex.
pub fn new_backup_id() -> Result<BackupId, CaptureBuildError> {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    BackupId::parse(format!("backup-{hex}")).map_err(|_| CaptureBuildError::Unavailable)
}

pub fn current_timestamp() -> Result<Timestamp, CaptureBuildError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CaptureBuildError::Unavailable)?
        .as_secs();
    format_unix_timestamp(seconds)
}

fn format_unix_timestamp(seconds: u64) -> Result<Timestamp, CaptureBuildError> {
    let days = i64::try_from(seconds / 86_400).map_err(|_| CaptureBuildError::Unavailable)?;
    let (year, month, day) = civil_date(days);
    let day_seconds = seconds % 86_400;
    Timestamp::parse(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        (day_seconds / 60) % 60,
        day_seconds % 60
    ))
    .map_err(|_| CaptureBuildError::Unavailable)
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
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

#[cfg(test)]
mod tests {
    use super::*;
    use guardian_core::{CredentialId, HostPin, PlanId, ProfileId, RepositoryId, SshEndpoint};

    fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&11_u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-ed25519");
        blob.push(1);
        Ok(VdsProfile {
            profile_id: ProfileId::parse("profile-001")?,
            label: "VDS".to_owned(),
            credential_id: CredentialId::parse("credential-001")?,
            endpoint: SshEndpoint {
                host: "vds.example".to_owned(),
                port: 22,
                user: "backup".to_owned(),
                host_pin: HostPin {
                    algorithm: "ssh-ed25519".to_owned(),
                    public_key_base64: base64_standard(&blob),
                },
            },
        })
    }

    fn base64_standard(bytes: &[u8]) -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.encode(bytes)
    }

    fn plan(database: Option<&str>) -> Result<FilesystemCapturePlan, Box<dyn std::error::Error>> {
        Ok(FilesystemCapturePlan {
            plan_id: PlanId::parse("plan-001")?,
            version: 1,
            profile_id: ProfileId::parse("profile-001")?,
            repository_id: RepositoryId::parse("repo-001")?,
            roots: vec!["/srv/app".to_owned(), "/etc/app".to_owned()],
            database_path: database.map(str::to_owned),
        })
    }

    fn requests(
        plan: &FilesystemCapturePlan,
        profile: &VdsProfile,
    ) -> Result<CaptureRequests, Box<dyn std::error::Error>> {
        let run_id = RunId::parse("run-001")?;
        Ok(build_capture_requests(CaptureInput {
            plan,
            plan_sha256: "abc123",
            source_layout: None,
            profile,
            run_id: &run_id,
            backup_id: BackupId::parse("backup-0001")?,
            created_at: Timestamp::parse("2026-10-07T12:00:00Z")?,
        })?)
    }

    #[test]
    fn the_same_plan_always_builds_the_same_requests() -> Result<(), Box<dyn std::error::Error>> {
        let (plan, profile) = (plan(Some("/srv/app/app.sqlite"))?, profile()?);
        let (first, second) = (requests(&plan, &profile)?, requests(&plan, &profile)?);
        assert_eq!(first.backup.manifest, second.backup.manifest);
        assert_eq!(first.backup.capture, second.backup.capture);
        assert_eq!(first.database, second.database);
        Ok(())
    }

    #[test]
    fn requests_carry_the_plan_roots_pinned_identity_and_payload_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let (plan, profile) = (plan(Some("/srv/app/app.sqlite"))?, profile()?);
        let built = requests(&plan, &profile)?;
        assert_eq!(built.backup.capture.roots, plan.roots);
        assert_eq!(
            built.backup.capture.payload_path.as_str(),
            FILESYSTEM_PAYLOAD_PATH
        );
        assert_eq!(built.backup.manifest.plan.sha256, "abc123");
        let database = built.database.ok_or("expected a database request")?;
        assert_eq!(database.database_path, "/srv/app/app.sqlite");
        assert_eq!(database.payload_path.as_str(), DATABASE_PAYLOAD_PATH);
        assert_eq!(database.run_id, built.backup.capture.run_id);
        Ok(())
    }

    #[test]
    fn a_plan_without_a_database_builds_no_database_request()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(requests(&plan(None)?, &profile()?)?.database.is_none());
        Ok(())
    }

    #[test]
    fn a_profile_that_is_not_the_plans_profile_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut other = profile()?;
        other.profile_id = ProfileId::parse("profile-other")?;
        let plan = plan(None)?;
        let run_id = RunId::parse("run-001")?;
        let result = build_capture_requests(CaptureInput {
            plan: &plan,
            plan_sha256: "x",
            source_layout: None,
            profile: &other,
            run_id: &run_id,
            backup_id: BackupId::parse("backup-0001")?,
            created_at: Timestamp::parse("2026-10-07T12:00:00Z")?,
        });
        assert!(matches!(result, Err(CaptureBuildError::Invalid)));
        Ok(())
    }

    #[test]
    fn backup_ids_are_unique_and_well_formed() -> Result<(), Box<dyn std::error::Error>> {
        let (first, second) = (new_backup_id()?, new_backup_id()?);
        assert_ne!(first, second);
        assert!(
            first.as_str().starts_with("backup-") && first.as_str().len() == "backup-".len() + 32
        );
        Ok(())
    }

    #[test]
    fn unix_seconds_format_as_utc_timestamps_including_leap_days()
    -> Result<(), Box<dyn std::error::Error>> {
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_709_208_000, "2024-02-29T12:00:00Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"),
            (1_791_374_400, "2026-10-07T12:00:00Z"),
        ] {
            assert_eq!(
                format_unix_timestamp(seconds)?.as_str(),
                expected,
                "{seconds}"
            );
        }
        Ok(())
    }
}
