//! Composition root for the embedded-database (SQLite) capture use case.
//! Mirrors `lib.rs`'s filesystem capture composition; produces its own
//! independently sealed backup rather than a combined multi-payload plan.

use fs2::available_space;
use guardian_archive::{ArchiveLimits, ZstdFileInspector};
use guardian_core::{
    AuditPort, CaptureRequestError, CaptureUseCaseError, EmbeddedDatabaseBackupRequest,
    EmbeddedDatabaseBackupUseCase, EmbeddedDatabaseCaptureRequest, ManifestSigner, SealedBackup,
    SecretStore, StoragePortError, VdsProfile,
};
use guardian_local_repository::{LocalRepository, LocalRepositoryStorageAdapter};
use guardian_ssh::{
    PinnedEmbeddedDatabaseCaptureAdapter, PinnedHost, SecretIdentityFile, SshUser, SystemOpenSsh,
};
use std::{fs, path::Path};
use tempfile::tempdir;

pub const MAX_DATABASE_SNAPSHOT_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const MINIMUM_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub(crate) const MAX_DISK_BUDGET_PROBE_BYTES: usize = 256;
pub(crate) const REMOTE_DATABASE_DISK_MARGIN_BYTES: u64 = 256 * 1024 * 1024;

pub struct EmbeddedDatabaseCaptureComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
    pub audit: &'a dyn AuditPort,
}

impl EmbeddedDatabaseCaptureComposition<'_> {
    pub fn execute(
        &self,
        request: EmbeddedDatabaseBackupRequest,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        self.validate_profile(&request.capture)?;
        self.require_encrypted_payload_path(&request.capture)?;
        self.require_disk_budget()?;
        let host = PinnedHost::parse(
            &self.profile.endpoint.host,
            self.profile.endpoint.port,
            &self.profile.endpoint.host_pin.algorithm,
            &self.profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::InvalidProfile))?;
        let user = SshUser::parse(&self.profile.endpoint.user)
            .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::InvalidProfile))?;
        let identity_file =
            SecretIdentityFile::from_store(self.credentials, &self.profile.credential_id).map_err(
                |_| CaptureUseCaseError::Capture(guardian_core::CapturePortError::Credential),
            )?;
        self.require_remote_disk_budget(
            &host,
            &user,
            identity_file.path(),
            &request.capture.database_path,
        )?;
        self.require_sqlite3(&host, &user, identity_file.path())?;
        let storage = LocalRepositoryStorageAdapter::encrypted(
            self.repository,
            request.manifest.backup_id.clone(),
            self.credentials,
        );
        let capture = PinnedEmbeddedDatabaseCaptureAdapter {
            ssh: self.ssh,
            host: &host,
            user: &user,
            identity_file: identity_file.path(),
            maximum_output_bytes: MAX_DATABASE_SNAPSHOT_BYTES,
        };
        // The capture stream is capped at MAX_DATABASE_SNAPSHOT_BYTES of
        // *compressed* network egress; a well-compressed source database can
        // expand well past that once decompressed, so inspection uses the
        // same generous expansion ceiling the filesystem archive path uses.
        let inspector = ZstdFileInspector::new(ArchiveLimits::conservative().max_expanded_bytes);
        EmbeddedDatabaseBackupUseCase {
            capture: &capture,
            storage: &storage,
            inspector: &inspector,
            signer,
            audit: self.audit,
        }
        .execute(request)
    }

    fn validate_profile(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
    ) -> Result<(), CaptureUseCaseError> {
        if self.profile.profile_id != request.profile_id {
            return Err(CaptureUseCaseError::Request(
                CaptureRequestError::ProfileMismatch,
            ));
        }
        self.profile
            .validate()
            .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::InvalidProfile))
    }

    fn require_encrypted_payload_path(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
    ) -> Result<(), CaptureUseCaseError> {
        request
            .payload_path
            .as_str()
            .ends_with(".enc")
            .then_some(())
            .ok_or(CaptureUseCaseError::Request(
                CaptureRequestError::InvalidPayloadPath,
            ))
    }

    fn require_sqlite3(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
    ) -> Result<(), CaptureUseCaseError> {
        let available = self
            .ssh
            .probe_sqlite3(host, user, identity_file)
            .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::PreflightFailed))?;
        available.then_some(()).ok_or(CaptureUseCaseError::Request(
            CaptureRequestError::PreflightFailed,
        ))
    }

    fn require_disk_budget(&self) -> Result<(), CaptureUseCaseError> {
        let available = available_space(self.repository.root())
            .map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Unavailable))?;
        (available >= MINIMUM_FREE_BYTES.saturating_add(MAX_DATABASE_SNAPSHOT_BYTES))
            .then_some(())
            .ok_or(CaptureUseCaseError::Storage(StoragePortError::Unavailable))
    }

    /// The `.backup` snapshot command writes a full uncompressed copy of the
    /// source database to a remote scratch file before compressing it — a
    /// large database on a nearly-full remote disk can otherwise fail
    /// minutes into a capture for a reason this cheap upfront check catches
    /// immediately.
    fn require_remote_disk_budget(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        database_path: &str,
    ) -> Result<(), CaptureUseCaseError> {
        let (size, free_kb) =
            probe_remote_disk_budget(self.ssh, host, user, identity_file, database_path)?;
        remote_disk_budget_is_sufficient(size, free_kb)
    }
}

pub(crate) fn probe_remote_disk_budget(
    ssh: &SystemOpenSsh,
    host: &PinnedHost,
    user: &SshUser,
    identity_file: &Path,
    database_path: &str,
) -> Result<(u64, u64), CaptureUseCaseError> {
    let temporary =
        tempdir().map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Unavailable))?;
    let destination = temporary.path().join("database-disk-budget.txt");
    let maximum_output_bytes = u64::try_from(MAX_DISK_BUDGET_PROBE_BYTES)
        .map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Unavailable))?;
    ssh.probe_database_disk_budget_to(
        host,
        user,
        identity_file,
        database_path,
        &destination,
        maximum_output_bytes,
    )
    .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::PreflightFailed))?;
    let bytes = fs::read(&destination)
        .map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Unavailable))?;
    parse_disk_budget_probe(&bytes)
}

pub(crate) fn remote_disk_budget_is_sufficient(
    size_bytes: u64,
    free_kb: u64,
) -> Result<(), CaptureUseCaseError> {
    let required_kb = size_bytes
        .saturating_add(REMOTE_DATABASE_DISK_MARGIN_BYTES)
        .div_ceil(1024);
    (free_kb >= required_kb)
        .then_some(())
        .ok_or(CaptureUseCaseError::Request(
            CaptureRequestError::PreflightFailed,
        ))
}

/// Parses the fixed probe's `"<size-bytes> <free-1k-blocks>"` stdout into
/// the two integers it reports; fails closed on anything else, including a
/// missing free-space value or extra trailing content.
pub(crate) fn parse_disk_budget_probe(bytes: &[u8]) -> Result<(u64, u64), CaptureUseCaseError> {
    if bytes.len() > MAX_DISK_BUDGET_PROBE_BYTES {
        return Err(CaptureUseCaseError::Request(
            CaptureRequestError::PreflightFailed,
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::PreflightFailed))?;
    let mut parts = text.trim().split_ascii_whitespace();
    let preflight_failed = || CaptureUseCaseError::Request(CaptureRequestError::PreflightFailed);
    let size = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(preflight_failed)?;
    let free_kb = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(preflight_failed)?;
    if parts.next().is_some() {
        return Err(preflight_failed());
    }
    Ok((size, free_kb))
}

#[cfg(test)]
mod tests {
    use super::EmbeddedDatabaseCaptureComposition;
    use super::{
        REMOTE_DATABASE_DISK_MARGIN_BYTES, parse_disk_budget_probe,
        remote_disk_budget_is_sufficient,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use guardian_core::{AuditPort, CaptureAuditCode};
    use guardian_core::{
        CaptureRequestError, CaptureUseCaseError, CredentialId, EmbeddedDatabaseCaptureRequest,
        HostPin, PayloadPath, ProfileId, RepositoryId, RunId, SecretStore, SecretStoreError,
        SecretValue, SshEndpoint, VdsProfile,
    };
    use guardian_local_repository::LocalRepository;
    use guardian_ssh::SystemOpenSsh;

    #[test]
    fn parse_disk_budget_probe_reads_the_fixed_two_integer_format() {
        assert_eq!(
            parse_disk_budget_probe(b"12345 67890\n"),
            Ok((12345, 67890))
        );
        assert_eq!(parse_disk_budget_probe(b"0 0"), Ok((0, 0)));
    }

    #[test]
    fn parse_disk_budget_probe_fails_closed_on_malformed_output() {
        assert!(parse_disk_budget_probe(b"").is_err());
        assert!(parse_disk_budget_probe(b"not-a-number 123").is_err());
        assert!(parse_disk_budget_probe(b"123").is_err());
        assert!(parse_disk_budget_probe(b"123 456 789").is_err());
        assert!(parse_disk_budget_probe(&vec![b'1'; 300]).is_err());
    }

    #[test]
    fn remote_disk_budget_is_sufficient_requires_the_file_size_plus_margin() {
        let size_bytes = 10 * 1024 * 1024 * 1024_u64;
        let exactly_enough_kb = (size_bytes + REMOTE_DATABASE_DISK_MARGIN_BYTES).div_ceil(1024);
        assert!(remote_disk_budget_is_sufficient(size_bytes, exactly_enough_kb).is_ok());
        assert!(remote_disk_budget_is_sufficient(size_bytes, exactly_enough_kb - 1).is_err());
    }

    #[test]
    fn capture_rejects_a_request_for_a_different_profile() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-001")?)?;
        let profile = profile()?;
        let audit = NoopAudit;
        let composition = EmbeddedDatabaseCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
        };
        let request = EmbeddedDatabaseCaptureRequest {
            run_id: RunId::parse("run-001")?,
            profile_id: ProfileId::parse("different-profile")?,
            database_path: "/srv/app/app.sqlite".to_owned(),
            payload_path: PayloadPath::parse("database.sqlite.zst")?,
        };
        assert!(matches!(
            composition.validate_profile(&request),
            Err(CaptureUseCaseError::Request(
                CaptureRequestError::ProfileMismatch
            ))
        ));
        Ok(())
    }

    #[test]
    fn capture_rejects_a_payload_path_without_the_encryption_suffix()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-002")?)?;
        let profile = profile()?;
        let audit = NoopAudit;
        let composition = EmbeddedDatabaseCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
        };
        let request = EmbeddedDatabaseCaptureRequest {
            run_id: RunId::parse("run-002")?,
            profile_id: profile.profile_id.clone(),
            database_path: "/srv/app/app.sqlite".to_owned(),
            payload_path: PayloadPath::parse("database.sqlite.zst")?,
        };
        assert!(matches!(
            composition.require_encrypted_payload_path(&request),
            Err(CaptureUseCaseError::Request(
                CaptureRequestError::InvalidPayloadPath
            ))
        ));
        Ok(())
    }

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
                host_pin: HostPin::parse("ssh-ed25519", STANDARD.encode(blob))?,
            },
        })
    }

    struct NoopAudit;

    impl AuditPort for NoopAudit {
        fn capture_failed(&self, _: &RunId, _: CaptureAuditCode) {}
    }

    struct NoopCredentialStore;

    impl SecretStore for NoopCredentialStore {
        fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(None)
        }

        fn store(&self, _: &CredentialId, _: &SecretValue) -> Result<(), SecretStoreError> {
            Ok(())
        }

        fn delete(&self, _: &CredentialId) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }
}
