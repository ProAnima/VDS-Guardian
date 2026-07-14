use crate::{ArchivePath, PayloadPath, ProfileId};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCaptureRequest {
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
    fn capture(
        &self,
        request: &FilesystemCaptureRequest,
    ) -> Result<CapturedStream, CapturePortError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStream {
    pub logical_role: String,
    pub archive_path: ArchivePath,
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
