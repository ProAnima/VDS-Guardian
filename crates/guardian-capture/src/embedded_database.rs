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
use std::path::Path;

pub const MAX_DATABASE_SNAPSHOT_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const MINIMUM_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

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
}

#[cfg(test)]
mod tests {
    use super::EmbeddedDatabaseCaptureComposition;
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
