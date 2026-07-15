//! Platform-independent domain contracts and use cases for VDS Guardian.

mod archive;
mod audit;
mod capture;
mod database;
mod database_connection;
mod docker;
mod enroll_profile;
mod host_trust;
mod identifiers;
mod manifest;
mod plan;
mod preflight;
mod profile;
mod profile_port;
mod restore;
mod retention;
mod secret;
mod signature;
mod state;
mod status;
mod storage;

pub use archive::{ArchiveInspectionPort, ArchiveInspectionPortError};
pub use audit::{AuditPort, CaptureAuditCode};
pub use capture::{
    CapturePortError, CaptureRequestError, CaptureUseCaseError, FilesystemBackupRequest,
    FilesystemBackupUseCase, FilesystemCapturePort, FilesystemCaptureRequest,
    FilesystemCaptureUseCase,
};
pub use database::{
    DatabaseCapability, DatabaseCapabilityProbeError, DatabaseCapabilityProbePort, DatabaseEngine,
    DatabasePreflightError, DatabasePreflightUseCase, DatabaseVersion,
};
pub use database_connection::{
    DatabaseAuthentication, DatabaseConnection, DatabaseConnectionError,
    DatabaseServerVersionProbeError, DatabaseServerVersionProbePort, VerifyDatabaseConnectionError,
    VerifyDatabaseConnectionUseCase,
};
pub use docker::{
    DiscoverDockerInventoryError, DiscoverDockerInventoryUseCase, DockerContainer,
    DockerContainerState, DockerHealth, DockerInventory, DockerInventoryError, DockerInventoryPort,
    DockerInventoryPortError, DockerMount, DockerMountKind, DockerNetwork,
};
pub use enroll_profile::{EnrollProfileError, EnrollProfileUseCase};
pub use host_trust::{
    HostKeyDiscoveryError, HostKeyDiscoveryPort, HostTrustError, TrustHostKeyUseCase,
};
pub use identifiers::{
    ArchivePath, BackupId, CredentialId, DatabaseId, IdentifierError, PayloadPath, PlanId,
    ProfileId, RepositoryId, RunId, Timestamp,
};
pub use manifest::{
    ConsistencyLevel, Manifest, ManifestError, PayloadEncryption, PayloadEntry, PlanReference,
    Producer, SignatureMetadata, SourceIdentity, VerificationState,
};
pub use plan::{CapturePlanError, FilesystemCapturePlan};
pub use preflight::{
    PreflightSshCaptureError, PreflightSshCaptureUseCase, SshCapabilityProbeError,
    SshCapabilityProbePort, SshCaptureCapabilities,
};
pub use profile::{HostPin, ProfileError, SshEndpoint, VdsProfile};
pub use profile_port::{ProfileStorePort, ProfileStorePortError};
pub use restore::{RestorePlan, RestorePlanError};
pub use retention::{
    RetentionOutcome, RetentionPlan, RetentionPlanError, RetentionPolicy, RetentionPolicyError,
    RetentionSnapshotEntry, build_retention_plan,
};
pub use secret::{SecretStore, SecretStoreError, SecretValue};
pub use signature::{ManifestSigner, ManifestVerifier, SignatureEnvelope, SigningError};
pub use state::BackupState;
pub use status::FoundationStatus;
pub use storage::{BackupStoragePort, SealedBackup, StoragePortError};
