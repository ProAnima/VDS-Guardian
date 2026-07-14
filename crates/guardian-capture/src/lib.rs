//! Capture pipeline: isolated staging, archive validation, then payload registration.

use guardian_archive::{ArchiveInspection, ArchiveLimits, inspect_tar_zstd};
use guardian_core::{PayloadEntry, PayloadPath};
use guardian_local_repository::{RepositoryError, StagingBackup};
use guardian_ssh::{PinnedHost, RemoteCapturePlan, SshUser, SystemOpenSsh};
use std::{fs, fs::File, path::Path};
use thiserror::Error;

pub trait FilesystemCaptureTransport {
    fn capture_to(&self, destination: &Path) -> Result<(), CaptureTransportError>;
}

pub struct PinnedSshTransport<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity_file: &'a Path,
    pub plan: &'a RemoteCapturePlan,
}

impl FilesystemCaptureTransport for PinnedSshTransport<'_> {
    fn capture_to(&self, destination: &Path) -> Result<(), CaptureTransportError> {
        self.ssh
            .capture_to(
                self.host,
                self.user,
                self.identity_file,
                self.plan,
                destination,
            )
            .map(|_| ())
            .map_err(|_| CaptureTransportError::Failed)
    }
}

pub fn capture_filesystem(
    staging: &StagingBackup<'_>,
    transport: &dyn FilesystemCaptureTransport,
    logical_role: &str,
    payload_path: PayloadPath,
    limits: ArchiveLimits,
) -> Result<CapturedPayload, CaptureError> {
    let destination = staging.reserve_payload_destination(&payload_path)?;
    if let Err(error) = transport.capture_to(&destination) {
        remove_partial(&destination);
        return Err(CaptureError::Transport(error));
    }
    let inspection = match File::open(&destination)
        .map_err(|_| CaptureError::Archive)
        .and_then(|file| inspect_tar_zstd(file, limits).map_err(|_| CaptureError::Archive))
    {
        Ok(inspection) => inspection,
        Err(error) => {
            remove_partial(&destination);
            return Err(error);
        }
    };
    let payload =
        match staging.register_payload_file(logical_role, payload_path, "application/zstd") {
            Ok(payload) => payload,
            Err(error) => {
                remove_partial(&destination);
                return Err(CaptureError::Repository(error));
            }
        };
    Ok(CapturedPayload {
        payload,
        inspection,
    })
}

fn remove_partial(destination: &Path) {
    let _ = fs::remove_file(destination);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPayload {
    pub payload: PayloadEntry,
    pub inspection: ArchiveInspection,
}

#[derive(Debug, Error)]
pub enum CaptureTransportError {
    #[error("capture transport failed")]
    Failed,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("capture transport failed")]
    Transport(#[source] CaptureTransportError),
    #[error("captured archive is invalid or violates archive policy")]
    Archive,
    #[error("repository rejected the staged payload")]
    Repository(#[from] RepositoryError),
}
