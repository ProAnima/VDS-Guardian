//! The `guardian-core` capture and capability ports, implemented over [`SystemOpenSsh`].

use crate::{PinnedHost, RemoteCapturePlan, SshIdentity, SshUser, SystemOpenSsh};
use guardian_core::{
    EmbeddedDatabaseCapturePort, EmbeddedDatabaseCaptureRequest, FilesystemCapturePort,
    FilesystemCaptureRequest, SecretStore, SshCapabilityProbeError, SshCapabilityProbePort,
    SshCaptureCapabilities, VdsProfile,
};
use std::path::Path;

pub struct PinnedSshCaptureAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity: &'a SshIdentity,
    pub maximum_output_bytes: u64,
}

pub struct PinnedSshCapabilityProbe<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
}

impl SshCapabilityProbePort for PinnedSshCapabilityProbe<'_> {
    fn probe(
        &self,
        profile: &VdsProfile,
    ) -> Result<SshCaptureCapabilities, SshCapabilityProbeError> {
        profile
            .validate()
            .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let host = PinnedHost::parse(
            &profile.endpoint.host,
            profile.endpoint.port,
            &profile.endpoint.host_pin.algorithm,
            &profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let user = SshUser::parse(&profile.endpoint.user)
            .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &profile.credential_id)
            .map_err(|_| SshCapabilityProbeError::Unavailable)?;
        let capabilities = self
            .ssh
            .probe_tar_zstd(&host, &user, &identity)
            .map_err(|_| SshCapabilityProbeError::Unavailable)?;
        Ok(SshCaptureCapabilities {
            tar_zstd: capabilities.tar_zstd,
        })
    }
}

impl FilesystemCapturePort for PinnedSshCaptureAdapter<'_> {
    fn capture_to(
        &self,
        request: &FilesystemCaptureRequest,
        destination: &Path,
    ) -> Result<(), guardian_core::CapturePortError> {
        let plan = RemoteCapturePlan::from_roots(request.roots.clone())
            .map_err(|_| guardian_core::CapturePortError::Transport)?;
        self.ssh
            .capture_to(
                self.host,
                self.user,
                self.identity,
                &plan,
                destination,
                self.maximum_output_bytes,
            )
            .map(|_| ())
            .map_err(|_| guardian_core::CapturePortError::Transport)
    }
}

pub struct PinnedEmbeddedDatabaseCaptureAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity: &'a SshIdentity,
    pub maximum_output_bytes: u64,
}

impl EmbeddedDatabaseCapturePort for PinnedEmbeddedDatabaseCaptureAdapter<'_> {
    fn capture_to(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
        destination: &Path,
    ) -> Result<(), guardian_core::CapturePortError> {
        self.ssh
            .snapshot_sqlite_to(
                self.host,
                self.user,
                self.identity,
                &request.database_path,
                destination,
                self.maximum_output_bytes,
            )
            .map(|_| ())
            .map_err(|_| guardian_core::CapturePortError::Transport)
    }
}
