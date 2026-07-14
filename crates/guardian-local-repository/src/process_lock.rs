use crate::RepositoryError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static HELD_REPOSITORIES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub(crate) struct ProcessLock {
    path: PathBuf,
}

impl ProcessLock {
    pub fn acquire(path: &Path) -> Result<Self, RepositoryError> {
        let mut held = registry().lock().map_err(|_| RepositoryError::Busy)?;
        if !held.insert(path.to_path_buf()) {
            return Err(RepositoryError::Busy);
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        if let Ok(mut held) = registry().lock() {
            held.remove(&self.path);
        }
    }
}

fn registry() -> &'static Mutex<HashSet<PathBuf>> {
    HELD_REPOSITORIES.get_or_init(|| Mutex::new(HashSet::new()))
}
