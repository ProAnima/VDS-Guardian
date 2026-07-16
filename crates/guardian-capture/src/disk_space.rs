use fs2::available_space;
use guardian_core::StoragePortError;
use std::path::Path;

/// Reads free space at the repository filesystem boundary. Kept as a small
/// port so capture can fail closed deterministically without manufacturing a
/// full disk in production or test environments.
pub trait DiskSpacePort: Send + Sync {
    fn available_space(&self, path: &Path) -> Result<u64, StoragePortError>;
}

pub struct SystemDiskSpace;

impl DiskSpacePort for SystemDiskSpace {
    fn available_space(&self, path: &Path) -> Result<u64, StoragePortError> {
        available_space(path).map_err(|_| StoragePortError::Unavailable)
    }
}

pub static SYSTEM_DISK_SPACE: SystemDiskSpace = SystemDiskSpace;
