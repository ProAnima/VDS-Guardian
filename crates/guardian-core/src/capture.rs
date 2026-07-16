use crate::{
    ArchiveInspectionPort, AuditPort, BackupStoragePort, CaptureAuditCode, Manifest, ManifestError,
    ManifestSigner, PayloadEntry, PayloadPath, ProfileId, RunId, SealedBackup, Timestamp,
};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCaptureRequest {
    pub run_id: RunId,
    pub profile_id: ProfileId,
    pub roots: Vec<String>,
    pub payload_path: PayloadPath,
}

#[derive(Debug, Clone)]
pub struct FilesystemBackupRequest {
    pub capture: FilesystemCaptureRequest,
    pub manifest: Manifest,
    pub sealed_at: Timestamp,
}

impl FilesystemCaptureRequest {
    pub fn validate(&self) -> Result<(), CaptureRequestError> {
        let roots_valid = !self.roots.is_empty()
            && self.roots.len() <= 32
            && self.roots.iter().all(|root| valid_remote_root(root));
        roots_valid
            .then_some(())
            .ok_or(CaptureRequestError::InvalidRoots)
    }
}

pub trait FilesystemCapturePort: Send + Sync {
    fn capture_to(
        &self,
        request: &FilesystemCaptureRequest,
        destination: &Path,
    ) -> Result<(), CapturePortError>;
}

pub struct FilesystemCaptureUseCase<'a> {
    pub capture: &'a dyn FilesystemCapturePort,
    pub storage: &'a dyn BackupStoragePort,
    pub inspector: &'a dyn ArchiveInspectionPort,
    pub audit: &'a dyn AuditPort,
}

impl FilesystemCaptureUseCase<'_> {
    pub fn execute(
        &self,
        request: &FilesystemCaptureRequest,
    ) -> Result<PayloadEntry, CaptureUseCaseError> {
        request.validate().map_err(CaptureUseCaseError::Request)?;
        self.storage
            .begin(&request.run_id)
            .map_err(CaptureUseCaseError::Storage)?;
        let destination = match self.storage.reserve(&request.payload_path) {
            Ok(destination) => destination,
            Err(error) => {
                return self.fail(
                    request,
                    CaptureAuditCode::Storage,
                    CaptureUseCaseError::Storage(error),
                );
            }
        };
        if let Err(error) = self.capture.capture_to(request, &destination) {
            return self.fail(
                request,
                CaptureAuditCode::Transport,
                CaptureUseCaseError::Capture(error),
            );
        }
        if self.inspector.inspect(&destination).is_err() {
            return self.fail(
                request,
                CaptureAuditCode::ArchivePolicy,
                CaptureUseCaseError::Archive,
            );
        }
        match self.storage.register_payload_path(
            "filesystem",
            request.payload_path.clone(),
            "application/zstd",
        ) {
            Ok(payload) => Ok(payload),
            Err(error) => self.fail(
                request,
                CaptureAuditCode::Storage,
                CaptureUseCaseError::Storage(error),
            ),
        }
    }

    fn fail<T>(
        &self,
        request: &FilesystemCaptureRequest,
        code: CaptureAuditCode,
        error: CaptureUseCaseError,
    ) -> Result<T, CaptureUseCaseError> {
        self.audit.capture_failed(&request.run_id, code);
        self.storage
            .discard(&request.run_id)
            .map_err(CaptureUseCaseError::Storage)?;
        Err(error)
    }
}

pub struct FilesystemBackupUseCase<'a> {
    pub capture: &'a dyn FilesystemCapturePort,
    pub storage: &'a dyn BackupStoragePort,
    pub inspector: &'a dyn ArchiveInspectionPort,
    pub signer: &'a dyn ManifestSigner,
    pub audit: &'a dyn AuditPort,
}

impl FilesystemBackupUseCase<'_> {
    pub fn execute(
        &self,
        request: FilesystemBackupRequest,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        let payload = FilesystemCaptureUseCase {
            capture: self.capture,
            storage: self.storage,
            inspector: self.inspector,
            audit: self.audit,
        }
        .execute(&request.capture)?;
        let mut manifest = request.manifest;
        if manifest.run_id != request.capture.run_id {
            return self.fail(
                &request.capture,
                CaptureUseCaseError::Manifest(ManifestError::NotSealed),
            );
        }
        if let Err(error) = manifest.add_payload(payload) {
            return self.fail(&request.capture, CaptureUseCaseError::Manifest(error));
        }
        self.storage
            .seal(manifest, request.sealed_at, self.signer)
            .map_err(|error| {
                self.audit
                    .capture_failed(&request.capture.run_id, CaptureAuditCode::Storage);
                let _ = self.storage.discard(&request.capture.run_id);
                CaptureUseCaseError::Storage(error)
            })
    }

    fn fail<T>(
        &self,
        request: &FilesystemCaptureRequest,
        error: CaptureUseCaseError,
    ) -> Result<T, CaptureUseCaseError> {
        self.audit
            .capture_failed(&request.run_id, CaptureAuditCode::Storage);
        self.storage
            .discard(&request.run_id)
            .map_err(CaptureUseCaseError::Storage)?;
        Err(error)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureRequestError {
    #[error("capture roots must be bounded absolute lexical paths")]
    InvalidRoots,
    #[error("capture request profile does not match the pinned SSH profile")]
    ProfileMismatch,
    #[error("pinned SSH profile is invalid")]
    InvalidProfile,
    #[error("required pinned SSH capture preflight failed")]
    PreflightFailed,
    #[error("capture payload path does not carry the required encryption suffix")]
    InvalidPayloadPath,
    #[error("embedded database path must be a bounded absolute lexical path")]
    InvalidDatabasePath,
    #[error("embedded database capture request does not match the filesystem capture request")]
    DatabaseRequestMismatch,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapturePortError {
    #[error("filesystem capture transport failed")]
    Transport,
    #[error("filesystem capture credential is unavailable or invalid")]
    Credential,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureUseCaseError {
    #[error("capture request is invalid")]
    Request(#[source] CaptureRequestError),
    #[error("capture transport failed")]
    Capture(#[source] CapturePortError),
    #[error("capture storage failed")]
    Storage(#[source] crate::StoragePortError),
    #[error("captured archive violates inspection policy")]
    Archive,
    #[error("backup manifest could not be finalized")]
    Manifest(#[source] ManifestError),
    #[error("repository has no configured recovery key; run `recovery init` first")]
    RecoveryKeyRequired,
}

pub(crate) fn valid_remote_root(root: &str) -> bool {
    root == "/"
        || (root.starts_with('/')
            && root.len() <= 1_024
            && !root.contains(['\0', '\n', '\r', '\\'])
            && root
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "" | "." | "..")))
}
