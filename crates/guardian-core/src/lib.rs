//! Platform-independent domain contracts and use cases for VDS Guardian.

mod archive;
mod audit;
mod capture;
mod enroll_profile;
mod host_trust;
mod identifiers;
mod manifest;
mod profile;
mod profile_port;
mod retention;
mod secret;
mod signature;
mod state;
mod status;
mod storage;

pub use archive::{ArchiveInspectionPort, ArchiveInspectionPortError};
pub use audit::{AuditPort, CaptureAuditCode};
pub use capture::{
    CapturePortError, CaptureRequestError, CaptureUseCaseError, FilesystemCapturePort,
    FilesystemCaptureRequest, FilesystemCaptureUseCase,
};
pub use enroll_profile::{EnrollProfileError, EnrollProfileUseCase};
pub use host_trust::{
    HostKeyDiscoveryError, HostKeyDiscoveryPort, HostTrustError, TrustHostKeyUseCase,
};
pub use identifiers::{
    ArchivePath, BackupId, CredentialId, IdentifierError, PayloadPath, PlanId, ProfileId,
    RepositoryId, RunId, Timestamp,
};
pub use manifest::{
    ConsistencyLevel, Manifest, ManifestError, PayloadEntry, PlanReference, Producer,
    SignatureMetadata, SourceIdentity, VerificationState,
};
pub use profile::{HostPin, ProfileError, SshEndpoint, VdsProfile};
pub use profile_port::{ProfileStorePort, ProfileStorePortError};
pub use retention::{RetentionPolicy, RetentionPolicyError};
pub use secret::{SecretStore, SecretStoreError, SecretValue};
pub use signature::{ManifestSigner, ManifestVerifier, SignatureEnvelope, SigningError};
pub use state::BackupState;
pub use status::FoundationStatus;
pub use storage::{BackupStoragePort, StoragePortError};
