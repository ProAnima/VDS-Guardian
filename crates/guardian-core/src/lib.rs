//! Platform-independent domain contracts and use cases for VDS Guardian.

mod identifiers;
mod manifest;
mod retention;
mod secret;
mod signature;
mod state;
mod status;

pub use identifiers::{
    ArchivePath, BackupId, CredentialId, IdentifierError, PayloadPath, PlanId, ProfileId,
    RepositoryId, RunId, Timestamp,
};
pub use manifest::{
    ConsistencyLevel, Manifest, ManifestError, PayloadEntry, PlanReference, Producer,
    SignatureMetadata, SourceIdentity, VerificationState,
};
pub use retention::{RetentionPolicy, RetentionPolicyError};
pub use secret::{SecretStore, SecretStoreError, SecretValue};
pub use signature::{ManifestSigner, ManifestVerifier, SignatureEnvelope, SigningError};
pub use state::BackupState;
pub use status::FoundationStatus;
