use crate::RepositoryError;
use guardian_core::CredentialId;
use rand_core::{OsRng, RngCore};

/// Mints a random `CredentialId` under the given prefix. Payload and recovery
/// keys share this random naming policy; identifiers are never derived from
/// caller input because a valid long input could exceed the identifier limit.
pub(crate) fn random_credential_id(prefix: &str) -> Result<CredentialId, RepositoryError> {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let id = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    CredentialId::parse(format!("{prefix}-{id}")).map_err(|_| RepositoryError::IntegrityFailure)
}
