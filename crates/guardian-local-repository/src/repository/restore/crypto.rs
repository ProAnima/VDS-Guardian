use super::*;

pub(super) fn decrypted_payload_reader(
    payload: &Path,
    payload_path: &PayloadPath,
    encryption: Option<&guardian_core::PayloadEncryption>,
    backup_id: &BackupId,
    secrets: &dyn SecretStore,
    recovery: Option<&PayloadKey>,
    scratch_root: &Path,
) -> Result<DecryptedPayload, RepositoryError> {
    let Some(encryption) = encryption else {
        let file = File::open(payload)
            .map_err(|error| RepositoryError::io("open restore payload", error))?;
        return Ok(DecryptedPayload::Direct(file));
    };
    let key = resolve_payload_key(encryption, backup_id, payload_path, secrets, recovery)?;
    let nonce = encryption.nonce()?;
    let temporary = tempfile::NamedTempFile::new_in(scratch_root)
        .map_err(|error| RepositoryError::io("create temporary decrypted payload", error))?;
    restrict_to_owner(temporary.path())?;
    let mut encrypted = File::open(payload)
        .map_err(|error| RepositoryError::io("open encrypted restore payload", error))?;
    let mut plaintext = temporary
        .reopen()
        .map_err(|error| RepositoryError::io("open temporary decrypted payload", error))?;
    decrypt_reader_to(
        &key,
        &mut encrypted,
        &mut plaintext,
        &associated_data(backup_id, payload_path),
        &nonce,
    )
    .map_err(|_| RepositoryError::Encryption)?;
    let file = temporary
        .reopen()
        .map_err(|error| RepositoryError::io("read temporary decrypted payload", error))?;
    Ok(DecryptedPayload::Temporary {
        _guard: temporary,
        file,
    })
}

/// Resolves one payload's data key: the primary `SecretStore` entry first,
/// falling back to unwrapping the manifest's own recovery-wrapped copy
/// (ADR 0013) only when the primary entry is unavailable and a repository
/// recovery key was supplied. Either fallback input being absent collapses
/// into the same `RepositoryError::Credential` this already returned before
/// recovery wrapping existed.
fn resolve_payload_key(
    encryption: &guardian_core::PayloadEncryption,
    backup_id: &BackupId,
    payload_path: &PayloadPath,
    secrets: &dyn SecretStore,
    recovery: Option<&PayloadKey>,
) -> Result<PayloadKey, RepositoryError> {
    let primary = secrets
        .load(&encryption.credential_id)
        .map_err(|_| RepositoryError::Credential)?;
    if let Some(secret) = primary {
        return PayloadKey::from_bytes(secret.expose()).map_err(|_| RepositoryError::Encryption);
    }
    let wrapped = encryption
        .recovery_wrapped_key()
        .map_err(|_| RepositoryError::Credential)?
        .ok_or(RepositoryError::Credential)?;
    let recovery_key = recovery.ok_or(RepositoryError::Credential)?;
    let mut raw_key = Vec::new();
    decrypt_self_describing_reader_to(
        recovery_key,
        &mut Cursor::new(wrapped),
        &mut raw_key,
        &recovery_wrap_associated_data(backup_id, payload_path),
    )
    .map_err(|_| RepositoryError::Credential)?;
    PayloadKey::from_bytes(&raw_key).map_err(|_| RepositoryError::Encryption)
}

pub(super) enum DecryptedPayload {
    Temporary {
        _guard: tempfile::NamedTempFile,
        file: File,
    },
    Direct(File),
}

impl DecryptedPayload {
    /// Measures the already-open file handle's real size — never the path —
    /// so this can never race a concurrent change to what the path itself
    /// names.
    pub(super) fn measured_len(&self) -> Result<u64, RepositoryError> {
        let file = match self {
            DecryptedPayload::Temporary { file, .. } => file,
            DecryptedPayload::Direct(file) => file,
        };
        file.metadata()
            .map(|metadata| metadata.len())
            .map_err(|source| RepositoryError::io("measure decrypted payload length", source))
    }
}

impl std::io::Read for DecryptedPayload {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DecryptedPayload::Temporary { file, .. } => file.read(buffer),
            DecryptedPayload::Direct(file) => file.read(buffer),
        }
    }
}
