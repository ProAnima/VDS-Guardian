use crate::{ProfileStorePort, ProfileStorePortError, VdsProfile};
use thiserror::Error;

pub struct EnrollProfileUseCase<'a> {
    pub store: &'a dyn ProfileStorePort,
}

impl EnrollProfileUseCase<'_> {
    pub fn execute(&self, profile: VdsProfile) -> Result<(), EnrollProfileError> {
        profile
            .validate()
            .map_err(|_| EnrollProfileError::InvalidProfile)?;
        self.store.save(profile).map_err(EnrollProfileError::Store)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnrollProfileError {
    #[error("profile is invalid")]
    InvalidProfile,
    #[error("profile storage failed")]
    Store(#[source] ProfileStorePortError),
}
