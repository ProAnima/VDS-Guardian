use crate::{ProfileId, ProfileStorePort, ProfileStorePortError, VdsProfile};
use thiserror::Error;

pub trait SshCapabilityProbePort: Send + Sync {
    fn probe(
        &self,
        profile: &VdsProfile,
    ) -> Result<SshCaptureCapabilities, SshCapabilityProbeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshCaptureCapabilities {
    pub tar_zstd: bool,
}

pub struct PreflightSshCaptureUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
    pub probe: &'a dyn SshCapabilityProbePort,
}

impl PreflightSshCaptureUseCase<'_> {
    pub fn execute(
        &self,
        profile_id: &ProfileId,
    ) -> Result<SshCaptureCapabilities, PreflightSshCaptureError> {
        let profile = self
            .profiles
            .get(profile_id)
            .map_err(PreflightSshCaptureError::ProfileStore)?
            .ok_or(PreflightSshCaptureError::ProfileNotFound)?;
        let capabilities = self
            .probe
            .probe(&profile)
            .map_err(PreflightSshCaptureError::Probe)?;
        capabilities
            .tar_zstd
            .then_some(capabilities)
            .ok_or(PreflightSshCaptureError::TarZstdUnsupported)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SshCapabilityProbeError {
    #[error("SSH capability probe rejected the profile")]
    Rejected,
    #[error("SSH capability probe is unavailable")]
    Unavailable,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreflightSshCaptureError {
    #[error("profile storage failed")]
    ProfileStore(#[source] ProfileStorePortError),
    #[error("profile was not found")]
    ProfileNotFound,
    #[error("SSH capability probe failed")]
    Probe(#[source] SshCapabilityProbeError),
    #[error("remote host does not support tar.zstd capture")]
    TarZstdUnsupported,
}
