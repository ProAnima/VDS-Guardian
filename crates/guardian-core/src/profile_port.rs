use crate::{ProfileId, VdsProfile};
use thiserror::Error;

pub trait ProfileStorePort: Send + Sync {
    fn save(&self, profile: VdsProfile) -> Result<(), ProfileStorePortError>;
    fn get(&self, profile_id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStorePortError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileStorePortError {
    #[error("profile storage is unavailable")]
    Unavailable,
    #[error("profile storage rejected the profile")]
    Rejected,
}
