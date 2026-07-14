//! Composition root for the core filesystem-capture use case.

use guardian_archive::{ArchiveLimits, TarZstdInspector};
use guardian_core::{
    AuditPort, CaptureUseCaseError, FilesystemCaptureRequest, FilesystemCaptureUseCase,
    PayloadEntry,
};
use guardian_local_repository::{LocalRepository, LocalRepositoryStorageAdapter};
use guardian_ssh::{PinnedHost, PinnedSshCaptureAdapter, SshUser, SystemOpenSsh};
use std::path::Path;

pub struct FilesystemCaptureComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity_file: &'a Path,
    pub audit: &'a dyn AuditPort,
    pub archive_limits: ArchiveLimits,
}

impl FilesystemCaptureComposition<'_> {
    pub fn execute(
        &self,
        request: &FilesystemCaptureRequest,
    ) -> Result<PayloadEntry, CaptureUseCaseError> {
        let storage = LocalRepositoryStorageAdapter::new(self.repository);
        let capture = PinnedSshCaptureAdapter {
            ssh: self.ssh,
            host: self.host,
            user: self.user,
            identity_file: self.identity_file,
        };
        let inspector = TarZstdInspector::new(self.archive_limits);
        FilesystemCaptureUseCase {
            capture: &capture,
            storage: &storage,
            inspector: &inspector,
            audit: self.audit,
        }
        .execute(request)
    }
}
