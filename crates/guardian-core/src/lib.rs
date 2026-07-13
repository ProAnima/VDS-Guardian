//! Platform-independent domain contracts and use cases for VDS Guardian.

mod identifiers;
mod manifest;
mod signature;
mod state;
mod status;

pub use identifiers::{
    BackupId, IdentifierError, PayloadPath, PlanId, ProfileId, RepositoryId, RunId, Timestamp,
};
pub use manifest::{
    ConsistencyLevel, Manifest, ManifestError, PayloadEntry, PlanReference, Producer,
    SignatureMetadata, SourceIdentity, VerificationState,
};
pub use signature::{ManifestSigner, SignatureEnvelope, SigningError};
pub use state::BackupState;
pub use status::FoundationStatus;
