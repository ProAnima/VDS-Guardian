use guardian_archive::ArchiveError;
use guardian_core::{ManifestError, RestorePlanError, SigningError};
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("repository is busy with another writer")]
    Busy,
    #[error("repository metadata is incompatible")]
    IncompatibleMetadata,
    #[error("staging run already exists")]
    StagingExists,
    #[error("sealed backup already exists")]
    BackupExists,
    #[error("staged payload path already exists")]
    PayloadExists,
    #[error("manifest does not belong to this staging run")]
    RunMismatch,
    #[error("manifest signer must use Ed25519")]
    UnsupportedSigner,
    #[error("staged payload verification failed")]
    IntegrityFailure,
    #[error("retention confirmation does not match the generated plan")]
    ConfirmationMismatch,
    #[error("retention plan belongs to another repository")]
    RepositoryMismatch,
    #[error("repository contents changed after the retention plan was created")]
    SnapshotChanged,
    #[error("retention audit record already exists")]
    AuditConflict,
    #[error("retention move failed and automatic rollback was incomplete")]
    RecoveryRequired,
    #[error("backups were quarantined but retention cleanup is still pending")]
    CleanupPending,
    #[error("filesystem boundary rejected a symlink or non-directory")]
    UnsafeFilesystemEntry,
    #[error("restore destination already exists")]
    RestoreDestinationExists,
    #[error("manifest contract rejected the backup")]
    Manifest(#[from] ManifestError),
    #[error("restore plan could not be created")]
    RestorePlan(#[source] RestorePlanError),
    #[error("restore extraction failed")]
    RestoreExtraction(#[source] ArchiveError),
    #[error("payload encryption could not be completed safely")]
    Encryption,
    #[error("the operating-system credential store is unavailable")]
    Credential,
    #[error("could not restrict a temporary file's permissions to the current user")]
    PermissionHardening,
    #[error("manifest signing or verification failed")]
    Signing(#[from] SigningError),
    #[error("repository I/O failed during {operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("repository metadata serialization failed")]
    Serialization,
}

impl RepositoryError {
    pub(crate) fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }
}
