use guardian_core::{ManifestError, SigningError};
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
    #[error("manifest does not belong to this staging run")]
    RunMismatch,
    #[error("manifest signer must use Ed25519")]
    UnsupportedSigner,
    #[error("staged payload verification failed")]
    IntegrityFailure,
    #[error("filesystem boundary rejected a symlink or non-directory")]
    UnsafeFilesystemEntry,
    #[error("manifest contract rejected the backup")]
    Manifest(#[from] ManifestError),
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
