use std::path::Path;
use thiserror::Error;

pub trait ArchiveInspectionPort: Send + Sync {
    fn inspect(&self, payload: &Path) -> Result<(), ArchiveInspectionPortError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArchiveInspectionPortError {
    #[error("archive violates inspection policy")]
    Rejected,
}
