use crate::{HostPin, ProfileStorePort, ProfileStorePortError, VdsProfile};
use thiserror::Error;

pub trait HostKeyDiscoveryPort: Send + Sync {
    fn discover(&self, host: &str, port: u16) -> Result<HostPin, HostKeyDiscoveryError>;
}

pub struct TrustHostKeyUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
}

impl TrustHostKeyUseCase<'_> {
    pub fn execute(
        &self,
        mut profile: VdsProfile,
        discovered: HostPin,
        confirmed: bool,
    ) -> Result<VdsProfile, HostTrustError> {
        if !confirmed {
            return Err(HostTrustError::ConfirmationRequired);
        }
        profile.endpoint.host_pin = discovered;
        profile
            .validate()
            .map_err(|_| HostTrustError::InvalidProfile)?;
        self.profiles
            .save(profile.clone())
            .map_err(HostTrustError::Store)?;
        Ok(profile)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostKeyDiscoveryError {
    #[error("host key discovery failed")]
    Failed,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostTrustError {
    #[error("explicit host-key confirmation is required")]
    ConfirmationRequired,
    #[error("profile is invalid")]
    InvalidProfile,
    #[error("profile storage failed")]
    Store(#[source] ProfileStorePortError),
}
