use super::{LocalRepository, RepositoryMetadata, RepositoryVerificationKey, random_credential_id};
use crate::RepositoryError;
use guardian_core::{CredentialId, SecretStore, SecretValue};
use guardian_encryption::PayloadKey;

impl LocalRepository {
    pub fn configure_recovery_key(
        &self,
        secrets: &dyn SecretStore,
    ) -> Result<CredentialId, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let mut metadata = self.read_metadata()?;
        if metadata.recovery_credential_id.is_some() {
            return Err(RepositoryError::RecoveryKeyAlreadyConfigured);
        }
        self.install_new_recovery_key(&mut metadata, secrets, PayloadKey::generate())
    }

    pub fn import_recovery_key(
        &self,
        secrets: &dyn SecretStore,
        key: PayloadKey,
    ) -> Result<CredentialId, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let mut metadata = self.read_metadata()?;
        if let Some(id) = metadata.recovery_credential_id.clone() {
            match secrets.load(&id).map_err(|_| RepositoryError::Credential)? {
                Some(existing) if existing.expose() == key.expose() => {}
                Some(_) => return Err(RepositoryError::RecoveryKeyMismatch),
                None => secrets
                    .store(&id, &SecretValue::new(key.expose().to_vec()))
                    .map_err(|_| RepositoryError::Credential)?,
            }
            return Ok(id);
        }
        self.install_new_recovery_key(&mut metadata, secrets, key)
    }

    pub fn pin_verification_key(
        &self,
        key: RepositoryVerificationKey,
    ) -> Result<(), RepositoryError> {
        let _lock = self.acquire_lock()?;
        let mut metadata = self.read_metadata()?;
        match metadata.trusted_verification_key.as_ref() {
            Some(existing) if existing == &key => return Ok(()),
            Some(_) => return Err(RepositoryError::TrustedSigningKeyMismatch),
            None => {}
        }
        metadata.trusted_verification_key = Some(key);
        self.write_metadata(&metadata)
    }

    pub fn trusted_verification_key(
        &self,
    ) -> Result<Option<RepositoryVerificationKey>, RepositoryError> {
        let _lock = self.acquire_lock()?;
        Ok(self.read_metadata()?.trusted_verification_key)
    }

    fn install_new_recovery_key(
        &self,
        metadata: &mut RepositoryMetadata,
        secrets: &dyn SecretStore,
        key: PayloadKey,
    ) -> Result<CredentialId, RepositoryError> {
        let id = random_credential_id("recovery")?;
        secrets
            .store(&id, &SecretValue::new(key.expose().to_vec()))
            .map_err(|_| RepositoryError::Credential)?;
        metadata.recovery_credential_id = Some(id.clone());
        if let Err(error) = self.write_metadata(metadata) {
            let _ = secrets.delete(&id);
            return Err(error);
        }
        Ok(id)
    }

    pub fn recovery_credential_id(&self) -> Result<Option<CredentialId>, RepositoryError> {
        let _lock = self.acquire_lock()?;
        self.recovery_credential_id_locked()
    }

    pub fn require_recovery_key(&self, secrets: &dyn SecretStore) -> Result<(), RepositoryError> {
        let _lock = self.acquire_lock()?;
        self.load_recovery_key(secrets)?
            .map(|_| ())
            .ok_or(RepositoryError::RecoveryKeyNotConfigured)
    }

    fn recovery_credential_id_locked(&self) -> Result<Option<CredentialId>, RepositoryError> {
        Ok(self.read_metadata()?.recovery_credential_id)
    }

    pub fn export_recovery_key(
        &self,
        secrets: &dyn SecretStore,
    ) -> Result<Option<PayloadKey>, RepositoryError> {
        self.load_recovery_key(secrets)
    }

    pub(crate) fn load_recovery_key(
        &self,
        secrets: &dyn SecretStore,
    ) -> Result<Option<PayloadKey>, RepositoryError> {
        let Some(id) = self.recovery_credential_id_locked()? else {
            return Ok(None);
        };
        let secret = secrets
            .load(&id)
            .map_err(|_| RepositoryError::Credential)?
            .ok_or(RepositoryError::Credential)?;
        PayloadKey::from_bytes(secret.expose())
            .map(Some)
            .map_err(|_| RepositoryError::Encryption)
    }
}
