use crate::process;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SshError {
    #[error("SSH host pin is invalid")]
    InvalidHostPin,
    #[error("SSH user is invalid")]
    InvalidUser,
    #[error("capture roots are invalid")]
    InvalidCapturePlan,
    #[error("capture destination is unavailable")]
    DestinationUnavailable,
    #[error("unable to prepare local SSH capture")]
    LocalIo,
    #[error("unable to start system OpenSSH")]
    LaunchFailed,
    #[error("remote capture failed")]
    CaptureFailed,
    #[error("SSH capture exceeded its total time limit")]
    TimedOut,
    #[error("SSH capture exceeded its idle stream time limit")]
    IdleTimedOut,
    #[error("SSH operation was cancelled by the operator")]
    Cancelled,
    #[error("the server did not present a usable public host key")]
    HostKeyUnavailable,
    #[error("SSH credential is unavailable")]
    CredentialUnavailable,
    #[error("SSH credential is not a supported SSH key, agent key or login password")]
    InvalidCredential,
    #[error("temporary SSH identity file could not be prepared")]
    TemporaryIdentityFile,
    #[error("the password helper is missing, so a password login cannot be used safely")]
    AskpassUnavailable,
    #[error("database connection is invalid")]
    InvalidDatabaseConnection,
    #[error("database authentication mode is not supported over SSH")]
    UnsupportedDatabaseAuthentication,
    #[error("pushed byte count did not match the payload's verified length")]
    ByteCountMismatch,
    #[error("replacement failed and the original data was restored")]
    ReplacementRolledBack,
    #[error("replacement rollback could not be completed safely")]
    ReplacementRollbackFailed,
}

pub(crate) fn map_wait_error(error: process::WaitError) -> SshError {
    match error {
        process::WaitError::TimedOut => SshError::TimedOut,
        process::WaitError::Cancelled => SshError::Cancelled,
        process::WaitError::Failed => SshError::LocalIo,
    }
}
