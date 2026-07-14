use crate::{
    ArchiveInspectionPort, AuditPort, BackupStoragePort, CaptureAuditCode, PayloadEntry,
    PayloadPath, ProfileId, RunId,
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
        let destination = self
            .storage
            .reserve(&request.payload_path)
            .map_err(CaptureUseCaseError::Storage)?;
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureRequestError {
    #[error("capture roots must be bounded absolute lexical paths")]
    InvalidRoots,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapturePortError {
    #[error("filesystem capture transport failed")]
    Transport,
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
}

fn valid_remote_root(root: &str) -> bool {
    root == "/"
        || (root.starts_with('/')
            && root.len() <= 1_024
            && !root.contains(['\0', '\n', '\r', '\\'])
            && root
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "" | "." | "..")))
}
