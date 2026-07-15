//! Capture and backup use cases for a lightweight embedded database (for
//! example SQLite) snapshotted from a single absolute file path, mirroring
//! `capture.rs`'s filesystem capture use cases exactly. PostgreSQL/MySQL
//! server dump/restore remain out of scope for the initial product.

use crate::capture::valid_remote_root;
use crate::{
    ArchiveInspectionPort, AuditPort, BackupStoragePort, CaptureAuditCode, CapturePortError,
    CaptureRequestError, CaptureUseCaseError, Manifest, ManifestError, ManifestSigner,
    PayloadEntry, PayloadPath, ProfileId, RunId, SealedBackup, Timestamp,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedDatabaseCaptureRequest {
    pub run_id: RunId,
    pub profile_id: ProfileId,
    pub database_path: String,
    pub payload_path: PayloadPath,
}

#[derive(Debug, Clone)]
pub struct EmbeddedDatabaseBackupRequest {
    pub capture: EmbeddedDatabaseCaptureRequest,
    pub manifest: Manifest,
    pub sealed_at: Timestamp,
}

impl EmbeddedDatabaseCaptureRequest {
    pub fn validate(&self) -> Result<(), CaptureRequestError> {
        (self.database_path != "/" && valid_remote_root(&self.database_path))
            .then_some(())
            .ok_or(CaptureRequestError::InvalidDatabasePath)
    }
}

pub trait EmbeddedDatabaseCapturePort: Send + Sync {
    fn capture_to(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
        destination: &Path,
    ) -> Result<(), CapturePortError>;
}

pub struct EmbeddedDatabaseCaptureUseCase<'a> {
    pub capture: &'a dyn EmbeddedDatabaseCapturePort,
    pub storage: &'a dyn BackupStoragePort,
    pub inspector: &'a dyn ArchiveInspectionPort,
    pub audit: &'a dyn AuditPort,
}

impl EmbeddedDatabaseCaptureUseCase<'_> {
    pub fn execute(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
    ) -> Result<PayloadEntry, CaptureUseCaseError> {
        request.validate().map_err(CaptureUseCaseError::Request)?;
        self.storage
            .begin(&request.run_id)
            .map_err(CaptureUseCaseError::Storage)?;
        self.execute_within_staging(request)
    }

    /// Same capture/inspect/register sequence as `execute`, but assumes the
    /// caller already opened the staging run — used when this payload is
    /// captured as the second of two into one combined backup, since
    /// `BackupStoragePort::begin` fails if called twice for the same run.
    pub fn execute_within_staging(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
    ) -> Result<PayloadEntry, CaptureUseCaseError> {
        request.validate().map_err(CaptureUseCaseError::Request)?;
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
                CaptureAuditCode::DatabasePolicy,
                CaptureUseCaseError::Archive,
            );
        }
        match self.storage.register_payload_path(
            "database",
            request.payload_path.clone(),
            "application/vnd.sqlite3+zstd",
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
        request: &EmbeddedDatabaseCaptureRequest,
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

pub struct EmbeddedDatabaseBackupUseCase<'a> {
    pub capture: &'a dyn EmbeddedDatabaseCapturePort,
    pub storage: &'a dyn BackupStoragePort,
    pub inspector: &'a dyn ArchiveInspectionPort,
    pub signer: &'a dyn ManifestSigner,
    pub audit: &'a dyn AuditPort,
}

impl EmbeddedDatabaseBackupUseCase<'_> {
    pub fn execute(
        &self,
        request: EmbeddedDatabaseBackupRequest,
    ) -> Result<SealedBackup, CaptureUseCaseError> {
        let payload = EmbeddedDatabaseCaptureUseCase {
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
        request: &EmbeddedDatabaseCaptureRequest,
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
