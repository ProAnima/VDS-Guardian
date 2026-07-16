//! Composition root for the core filesystem-capture use case.

mod disk_space;
mod embedded_database;

use guardian_archive::{ArchiveLimits, TarZstdInspector, ZstdFileInspector};
use guardian_core::{
    AuditPort, BackupId, BackupStoragePort, CaptureAuditCode, CaptureRequestError,
    CaptureUseCaseError, EmbeddedDatabaseCaptureRequest, EmbeddedDatabaseCaptureUseCase,
    FilesystemBackupRequest, FilesystemBackupUseCase, FilesystemCaptureRequest,
    FilesystemCaptureUseCase, ManifestError, ManifestSigner, PayloadEntry, RunId, SealedBackup,
    SecretStore, SshCapabilityProbePort, StoragePortError, VdsProfile,
};
use guardian_local_repository::{LocalRepository, LocalRepositoryStorageAdapter};
use guardian_ssh::{
    PinnedEmbeddedDatabaseCaptureAdapter, PinnedHost, PinnedSshCapabilityProbe,
    PinnedSshCaptureAdapter, SshIdentity, SshUser, SystemOpenSsh,
};
use std::path::Path;

pub use disk_space::{DiskSpacePort, SYSTEM_DISK_SPACE};
pub use embedded_database::{EmbeddedDatabaseCaptureComposition, MAX_DATABASE_SNAPSHOT_BYTES};
use embedded_database::{probe_remote_disk_budget, remote_disk_budget_is_sufficient};

pub const MAX_CAPTURE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const MINIMUM_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

pub struct FilesystemCaptureComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
    pub audit: &'a dyn AuditPort,
    pub disk_space: &'a dyn DiskSpacePort,
    pub archive_limits: ArchiveLimits,
}

impl FilesystemCaptureComposition<'_> {
    /// Writes the started/sealed/cancelled/failed audit trail itself —
    /// mandatory, not a responsibility left to whichever caller happens to
    /// invoke this (today only the desktop app; a future CLI capture
    /// command must not be able to skip it either). Mirrors
    /// `DeploymentComposition::execute`'s identical shape: "started" is
    /// strict, "cancelled"/"failed" are best-effort, "sealed" is strict —
    /// matching every caller's own prior behavior before this became the
    /// composition's job. Reuses `request.capture.run_id` rather than
    /// taking a new parameter — it is already the correlation id this
    /// exact run was constructed with.
    pub fn execute(
        &self,
        request: FilesystemBackupRequest,
        database: Option<EmbeddedDatabaseCaptureRequest>,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        let run_id = request.capture.run_id.clone();
        self.write_audit(&run_id, "started", None)?;
        let result = match database {
            Some(database) => self.execute_combined(request, database, signer),
            None => self.execute_filesystem_only(request, signer),
        };
        match &result {
            Ok(sealed) => self.write_audit(&run_id, "sealed", Some(&sealed.backup_id))?,
            Err(_) if self.ssh.is_cancelled() => {
                let _ = self.write_audit(&run_id, "cancelled", None);
            }
            Err(_) => {
                let _ = self.write_audit(&run_id, "failed", None);
            }
        }
        result
    }

    fn write_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: Option<&BackupId>,
    ) -> Result<(), CaptureUseCaseError> {
        self.repository
            .write_capture_audit(run_id, state, backup_id)
            .map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Rejected))
    }

    /// Unchanged from before `database` existed: exactly today's single-
    /// payload capture, delegating wholesale to `FilesystemBackupUseCase`.
    fn execute_filesystem_only(
        &self,
        request: FilesystemBackupRequest,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        self.validate_profile(&request.capture)?;
        self.require_encrypted_payload_path(&request.capture)?;
        self.require_recovery_key()?;
        self.require_preflight()?;
        self.require_disk_budget(false)?;
        let host = self.pinned_host()?;
        let user = self.ssh_user()?;
        let identity_file = self.identity_file()?;
        let storage = LocalRepositoryStorageAdapter::encrypted(
            self.repository,
            request.manifest.backup_id.clone(),
            self.credentials,
        );
        let capture = PinnedSshCaptureAdapter {
            ssh: self.ssh,
            host: &host,
            user: &user,
            identity_file: identity_file.path(),
            maximum_output_bytes: MAX_CAPTURE_BYTES,
        };
        let inspector = TarZstdInspector::new(self.archive_limits);
        FilesystemBackupUseCase {
            capture: &capture,
            storage: &storage,
            inspector: &inspector,
            signer,
            audit: self.audit,
        }
        .execute(request)
    }

    /// Captures the filesystem payload, then (in the same staging run, since
    /// `BackupStoragePort::begin` fails if called twice) the database
    /// payload, before sealing once with both entries in one manifest.
    fn execute_combined(
        &self,
        request: FilesystemBackupRequest,
        database: EmbeddedDatabaseCaptureRequest,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        self.validate_profile(&request.capture)?;
        self.require_encrypted_payload_path(&request.capture)?;
        self.require_recovery_key()?;
        self.require_preflight()?;
        self.require_matching_database_request(&request.capture, &database)?;
        self.require_disk_budget(true)?;
        let host = self.pinned_host()?;
        let user = self.ssh_user()?;
        let identity_file = self.identity_file()?;
        self.require_remote_disk_budget(
            &host,
            &user,
            identity_file.path(),
            &database.database_path,
        )?;
        self.require_sqlite3(&host, &user, identity_file.path())?;
        let storage = LocalRepositoryStorageAdapter::encrypted(
            self.repository,
            request.manifest.backup_id.clone(),
            self.credentials,
        );
        let capture = PinnedSshCaptureAdapter {
            ssh: self.ssh,
            host: &host,
            user: &user,
            identity_file: identity_file.path(),
            maximum_output_bytes: MAX_CAPTURE_BYTES,
        };
        let inspector = TarZstdInspector::new(self.archive_limits);
        let filesystem_payload = FilesystemCaptureUseCase {
            capture: &capture,
            storage: &storage,
            inspector: &inspector,
            audit: self.audit,
        }
        .execute(&request.capture)?;
        let database_capture = PinnedEmbeddedDatabaseCaptureAdapter {
            ssh: self.ssh,
            host: &host,
            user: &user,
            identity_file: identity_file.path(),
            maximum_output_bytes: MAX_DATABASE_SNAPSHOT_BYTES,
        };
        let database_inspector =
            ZstdFileInspector::new(ArchiveLimits::conservative().max_expanded_bytes);
        let database_payload = EmbeddedDatabaseCaptureUseCase {
            capture: &database_capture,
            storage: &storage,
            inspector: &database_inspector,
            audit: self.audit,
        }
        .execute_within_staging(&database)?;
        self.seal_combined(
            &storage,
            request,
            filesystem_payload,
            database_payload,
            signer,
        )
    }

    fn seal_combined(
        &self,
        storage: &LocalRepositoryStorageAdapter<'_>,
        request: FilesystemBackupRequest,
        filesystem_payload: PayloadEntry,
        database_payload: PayloadEntry,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        let mut manifest = request.manifest;
        let run_id = request.capture.run_id;
        if manifest.run_id != run_id {
            return self.fail_combined(
                &run_id,
                storage,
                CaptureUseCaseError::Manifest(ManifestError::NotSealed),
            );
        }
        if let Err(error) = manifest.add_payload(filesystem_payload) {
            return self.fail_combined(&run_id, storage, CaptureUseCaseError::Manifest(error));
        }
        if let Err(error) = manifest.add_payload(database_payload) {
            return self.fail_combined(&run_id, storage, CaptureUseCaseError::Manifest(error));
        }
        storage
            .seal(manifest, request.sealed_at, signer)
            .map_err(|error| {
                self.audit
                    .capture_failed(&run_id, CaptureAuditCode::Storage);
                let _ = storage.discard(&run_id);
                CaptureUseCaseError::Storage(error)
            })
    }

    fn fail_combined<T>(
        &self,
        run_id: &RunId,
        storage: &LocalRepositoryStorageAdapter<'_>,
        error: CaptureUseCaseError,
    ) -> Result<T, CaptureUseCaseError> {
        self.audit.capture_failed(run_id, CaptureAuditCode::Storage);
        storage
            .discard(run_id)
            .map_err(CaptureUseCaseError::Storage)?;
        Err(error)
    }

    fn pinned_host(&self) -> Result<PinnedHost, CaptureUseCaseError> {
        PinnedHost::parse(
            &self.profile.endpoint.host,
            self.profile.endpoint.port,
            &self.profile.endpoint.host_pin.algorithm,
            &self.profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::InvalidProfile))
    }

    fn ssh_user(&self) -> Result<SshUser, CaptureUseCaseError> {
        SshUser::parse(&self.profile.endpoint.user)
            .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::InvalidProfile))
    }

    fn identity_file(&self) -> Result<SshIdentity, CaptureUseCaseError> {
        SshIdentity::from_store(self.credentials, &self.profile.credential_id)
            .map_err(|_| CaptureUseCaseError::Capture(guardian_core::CapturePortError::Credential))
    }

    fn require_matching_database_request(
        &self,
        request: &FilesystemCaptureRequest,
        database: &EmbeddedDatabaseCaptureRequest,
    ) -> Result<(), CaptureUseCaseError> {
        (database.run_id == request.run_id && database.profile_id == request.profile_id)
            .then_some(())
            .ok_or(CaptureUseCaseError::Request(
                CaptureRequestError::DatabaseRequestMismatch,
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

    fn validate_profile(
        &self,
        request: &FilesystemCaptureRequest,
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
        request: &FilesystemCaptureRequest,
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

    fn require_preflight(&self) -> Result<(), CaptureUseCaseError> {
        let capabilities = PinnedSshCapabilityProbe {
            ssh: self.ssh,
            credentials: self.credentials,
        }
        .probe(self.profile)
        .map_err(|_| CaptureUseCaseError::Request(CaptureRequestError::PreflightFailed))?;
        capabilities
            .tar_zstd
            .then_some(())
            .ok_or(CaptureUseCaseError::Request(
                CaptureRequestError::PreflightFailed,
            ))
    }

    /// Every new payload must be recovery-wrapped (ADR 0013); checked here,
    /// before any network round-trip, so a repository that never ran
    /// `recovery init` fails fast rather than sealing a backup only the
    /// current OS keyring can ever decrypt. `staging.rs` re-checks this
    /// itself as the real fail-closed guard — this is the fast preflight.
    fn require_recovery_key(&self) -> Result<(), CaptureUseCaseError> {
        self.repository
            .require_recovery_key(self.credentials)
            .map_err(|_| CaptureUseCaseError::RecoveryKeyRequired)
    }

    fn require_disk_budget(&self, include_database: bool) -> Result<(), CaptureUseCaseError> {
        let available = self
            .disk_space
            .available_space(self.repository.root())
            .map_err(CaptureUseCaseError::Storage)?;
        let mut required = MINIMUM_FREE_BYTES.saturating_add(MAX_CAPTURE_BYTES);
        if include_database {
            required = required.saturating_add(MAX_DATABASE_SNAPSHOT_BYTES);
        }
        (available >= required)
            .then_some(())
            .ok_or(CaptureUseCaseError::Storage(StoragePortError::Unavailable))
    }
}

#[cfg(test)]
mod tests {
    use super::{FilesystemCaptureComposition, SYSTEM_DISK_SPACE};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use guardian_archive::ArchiveLimits;
    use guardian_core::{
        AuditPort, CaptureAuditCode, CaptureRequestError, CaptureUseCaseError, CredentialId,
        FilesystemCaptureRequest, HostPin, PayloadPath, ProfileId, RepositoryId, RunId,
        SecretStore, SecretStoreError, SecretValue, SshEndpoint, VdsProfile,
    };
    use guardian_local_repository::LocalRepository;
    use guardian_ssh::SystemOpenSsh;

    #[test]
    fn capture_rejects_a_request_for_a_different_profile() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-001")?)?;
        let profile = profile()?;
        let audit = NoopAudit;
        let composition = FilesystemCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
            disk_space: &SYSTEM_DISK_SPACE,
            archive_limits: ArchiveLimits::conservative(),
        };
        let request = FilesystemCaptureRequest {
            run_id: RunId::parse("run-001")?,
            profile_id: ProfileId::parse("different-profile")?,
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("filesystem.tar.zst")?,
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
        let composition = FilesystemCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
            disk_space: &SYSTEM_DISK_SPACE,
            archive_limits: ArchiveLimits::conservative(),
        };
        let request = FilesystemCaptureRequest {
            run_id: RunId::parse("run-002")?,
            profile_id: profile.profile_id.clone(),
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("filesystem.tar.zst")?,
        };
        assert!(matches!(
            composition.require_encrypted_payload_path(&request),
            Err(CaptureUseCaseError::Request(
                CaptureRequestError::InvalidPayloadPath
            ))
        ));
        Ok(())
    }

    #[test]
    fn combined_capture_rejects_a_database_request_for_a_different_run_before_touching_ssh()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-003")?)?;
        let profile = profile()?;
        let audit = NoopAudit;
        let composition = FilesystemCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
            disk_space: &SYSTEM_DISK_SPACE,
            archive_limits: ArchiveLimits::conservative(),
        };
        let request = FilesystemCaptureRequest {
            run_id: RunId::parse("run-003")?,
            profile_id: profile.profile_id.clone(),
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("filesystem.tar.zst")?,
        };
        let mismatched_run = guardian_core::EmbeddedDatabaseCaptureRequest {
            run_id: RunId::parse("a-different-run")?,
            profile_id: profile.profile_id.clone(),
            database_path: "/srv/app/app.sqlite".to_owned(),
            payload_path: PayloadPath::parse("database.sqlite.zst")?,
        };
        assert!(matches!(
            composition.require_matching_database_request(&request, &mismatched_run),
            Err(CaptureUseCaseError::Request(
                CaptureRequestError::DatabaseRequestMismatch
            ))
        ));
        let mismatched_profile = guardian_core::EmbeddedDatabaseCaptureRequest {
            run_id: request.run_id.clone(),
            profile_id: ProfileId::parse("a-different-profile")?,
            database_path: "/srv/app/app.sqlite".to_owned(),
            payload_path: PayloadPath::parse("database.sqlite.zst")?,
        };
        assert!(matches!(
            composition.require_matching_database_request(&request, &mismatched_profile),
            Err(CaptureUseCaseError::Request(
                CaptureRequestError::DatabaseRequestMismatch
            ))
        ));
        Ok(())
    }

    #[test]
    fn recovery_preflight_rejects_a_dangling_credential_reference()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-dangling")?)?;
        repository.configure_recovery_key(&NoopCredentialStore)?;
        let profile = profile()?;
        let audit = NoopAudit;
        let composition = FilesystemCaptureComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::default(),
            profile: &profile,
            credentials: &NoopCredentialStore,
            audit: &audit,
            disk_space: &SYSTEM_DISK_SPACE,
            archive_limits: ArchiveLimits::conservative(),
        };
        assert!(matches!(
            composition.require_recovery_key(),
            Err(CaptureUseCaseError::RecoveryKeyRequired)
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
