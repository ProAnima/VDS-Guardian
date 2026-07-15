//! Composition root for the core filesystem-capture use case.

use fs2::available_space;
use guardian_archive::{ArchiveLimits, TarZstdInspector};
use guardian_core::{
    AuditPort, CaptureRequestError, CaptureUseCaseError, FilesystemBackupRequest,
    FilesystemBackupUseCase, FilesystemCaptureRequest, ManifestSigner, SealedBackup, SecretStore,
    SshCapabilityProbePort, StoragePortError, VdsProfile,
};
use guardian_local_repository::{LocalRepository, LocalRepositoryStorageAdapter};
use guardian_ssh::{
    PinnedHost, PinnedSshCapabilityProbe, PinnedSshCaptureAdapter, SecretIdentityFile, SshUser,
    SystemOpenSsh,
};

pub const MAX_CAPTURE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const MINIMUM_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

pub struct FilesystemCaptureComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
    pub audit: &'a dyn AuditPort,
    pub archive_limits: ArchiveLimits,
}

impl FilesystemCaptureComposition<'_> {
    pub fn execute(
        &self,
        request: FilesystemBackupRequest,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        self.validate_profile(&request.capture)?;
        self.require_encrypted_payload_path(&request.capture)?;
        self.require_preflight()?;
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

    fn require_disk_budget(&self) -> Result<(), CaptureUseCaseError> {
        let available = available_space(self.repository.root())
            .map_err(|_| CaptureUseCaseError::Storage(StoragePortError::Unavailable))?;
        (available >= MINIMUM_FREE_BYTES.saturating_add(MAX_CAPTURE_BYTES))
            .then_some(())
            .ok_or(CaptureUseCaseError::Storage(StoragePortError::Unavailable))
    }
}

#[cfg(test)]
mod tests {
    use super::FilesystemCaptureComposition;
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
