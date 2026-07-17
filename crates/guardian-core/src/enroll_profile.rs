use crate::{
    ProfileStorePort, ProfileStorePortError, SecretStore, SecretStoreError, SecretValue,
    SshCapabilityProbeError, SshCapabilityProbePort, VdsProfile,
};
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

pub struct EnrollVerifiedProfileUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
    pub secrets: &'a dyn SecretStore,
    pub probe: &'a dyn SshCapabilityProbePort,
}

impl EnrollVerifiedProfileUseCase<'_> {
    pub fn execute(
        &self,
        profile: VdsProfile,
        secret: &SecretValue,
    ) -> Result<(), EnrollVerifiedProfileError> {
        profile
            .validate()
            .map_err(|_| EnrollVerifiedProfileError::InvalidProfile)?;
        let credential_id = profile.credential_id.clone();
        if self
            .secrets
            .load(&credential_id)
            .map_err(EnrollVerifiedProfileError::SecretStore)?
            .is_some()
        {
            return Err(EnrollVerifiedProfileError::CredentialExists);
        }
        if let Err(source) = self.secrets.store(&credential_id, secret) {
            if self.secrets.delete(&credential_id).is_err() {
                return Err(EnrollVerifiedProfileError::Cleanup);
            }
            return Err(EnrollVerifiedProfileError::SecretStore(source));
        }
        let result = self.verify_and_save(profile);
        if result.is_err() && self.secrets.delete(&credential_id).is_err() {
            return Err(EnrollVerifiedProfileError::Cleanup);
        }
        result
    }

    fn verify_and_save(&self, profile: VdsProfile) -> Result<(), EnrollVerifiedProfileError> {
        let capabilities = self
            .probe
            .probe(&profile)
            .map_err(EnrollVerifiedProfileError::Probe)?;
        if !capabilities.tar_zstd {
            return Err(EnrollVerifiedProfileError::TarZstdUnsupported);
        }
        self.profiles
            .save(profile)
            .map_err(EnrollVerifiedProfileError::ProfileStore)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnrollProfileError {
    #[error("profile is invalid")]
    InvalidProfile,
    #[error("profile storage failed")]
    Store(#[source] ProfileStorePortError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnrollVerifiedProfileError {
    #[error("profile is invalid")]
    InvalidProfile,
    #[error("credential already exists")]
    CredentialExists,
    #[error("secret storage failed")]
    SecretStore(#[source] SecretStoreError),
    #[error("SSH capability probe failed")]
    Probe(#[source] SshCapabilityProbeError),
    #[error("remote host does not support tar.zstd capture")]
    TarZstdUnsupported,
    #[error("profile storage failed")]
    ProfileStore(#[source] ProfileStorePortError),
    #[error("failed enrollment could not remove its staged credential")]
    Cleanup,
}

#[cfg(test)]
mod tests {
    use super::{EnrollVerifiedProfileError, EnrollVerifiedProfileUseCase};
    use crate::{
        CredentialId, HostPin, ProfileId, ProfileStorePort, ProfileStorePortError, SecretStore,
        SecretStoreError, SecretValue, SshCapabilityProbeError, SshCapabilityProbePort,
        SshCaptureCapabilities, SshEndpoint, VdsProfile,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::sync::Mutex;

    #[test]
    fn failed_preflight_removes_the_staged_credential_and_publishes_no_profile()
    -> Result<(), Box<dyn std::error::Error>> {
        let profiles = Profiles::default();
        let secrets = Secrets::default();
        let result = use_case(&profiles, &secrets, &Probe { succeeds: false })
            .execute(profile()?, &SecretValue::new(b"private-key".to_vec()));

        assert!(matches!(result, Err(EnrollVerifiedProfileError::Probe(_))));
        assert!(secrets.value()?.is_none());
        assert!(profiles.values()?.is_empty());
        Ok(())
    }

    #[test]
    fn failed_profile_commit_removes_the_staged_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let profiles = Profiles {
            fail_save: true,
            ..Profiles::default()
        };
        let secrets = Secrets::default();
        let result = use_case(&profiles, &secrets, &Probe { succeeds: true })
            .execute(profile()?, &SecretValue::new(b"private-key".to_vec()));

        assert!(matches!(
            result,
            Err(EnrollVerifiedProfileError::ProfileStore(_))
        ));
        assert!(secrets.value()?.is_none());
        Ok(())
    }

    #[test]
    fn successful_preflight_commits_the_profile_and_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let profiles = Profiles::default();
        let secrets = Secrets::default();
        use_case(&profiles, &secrets, &Probe { succeeds: true })
            .execute(profile()?, &SecretValue::new(b"private-key".to_vec()))?;

        assert_eq!(secrets.value()?.as_deref(), Some(b"private-key".as_slice()));
        assert_eq!(profiles.values()?.len(), 1);
        Ok(())
    }

    #[test]
    fn partial_secret_store_failure_is_cleaned_up() -> Result<(), Box<dyn std::error::Error>> {
        let profiles = Profiles::default();
        let secrets = Secrets {
            fail_store_after_write: true,
            ..Secrets::default()
        };
        let result = use_case(&profiles, &secrets, &Probe { succeeds: true })
            .execute(profile()?, &SecretValue::new(b"private-key".to_vec()));

        assert!(matches!(
            result,
            Err(EnrollVerifiedProfileError::SecretStore(_))
        ));
        assert!(secrets.value()?.is_none());
        assert!(profiles.values()?.is_empty());
        Ok(())
    }

    #[test]
    fn cleanup_failure_is_a_distinct_hard_error() -> Result<(), Box<dyn std::error::Error>> {
        let profiles = Profiles::default();
        let secrets = Secrets {
            fail_delete: true,
            ..Secrets::default()
        };
        let result = use_case(&profiles, &secrets, &Probe { succeeds: false })
            .execute(profile()?, &SecretValue::new(b"private-key".to_vec()));

        assert_eq!(result, Err(EnrollVerifiedProfileError::Cleanup));
        assert!(secrets.value()?.is_some());
        assert!(profiles.values()?.is_empty());
        Ok(())
    }

    fn use_case<'a>(
        profiles: &'a Profiles,
        secrets: &'a Secrets,
        probe: &'a Probe,
    ) -> EnrollVerifiedProfileUseCase<'a> {
        EnrollVerifiedProfileUseCase {
            profiles,
            secrets,
            probe,
        }
    }

    fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
        let mut key = Vec::from(11_u32.to_be_bytes());
        key.extend_from_slice(b"ssh-ed25519");
        key.push(1);
        Ok(VdsProfile {
            profile_id: ProfileId::parse("profile-1")?,
            label: "VDS".to_owned(),
            endpoint: SshEndpoint {
                host: "vds.example".to_owned(),
                port: 22,
                user: "backup".to_owned(),
                host_pin: HostPin::parse("ssh-ed25519", STANDARD.encode(key))?,
            },
            credential_id: CredentialId::parse("credential-1")?,
        })
    }

    #[derive(Default)]
    struct Profiles {
        values: Mutex<Vec<VdsProfile>>,
        fail_save: bool,
    }
    impl Profiles {
        fn values(
            &self,
        ) -> Result<std::sync::MutexGuard<'_, Vec<VdsProfile>>, ProfileStorePortError> {
            self.values
                .lock()
                .map_err(|_| ProfileStorePortError::Unavailable)
        }
    }
    impl ProfileStorePort for Profiles {
        fn save(&self, profile: VdsProfile) -> Result<(), ProfileStorePortError> {
            if self.fail_save {
                return Err(ProfileStorePortError::Unavailable);
            }
            self.values()?.push(profile);
            Ok(())
        }
        fn get(&self, id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStorePortError> {
            Ok(self
                .values()?
                .iter()
                .find(|profile| &profile.profile_id == id)
                .cloned())
        }
    }

    #[derive(Default)]
    struct Secrets {
        value: Mutex<Option<Vec<u8>>>,
        fail_store_after_write: bool,
        fail_delete: bool,
    }
    impl Secrets {
        fn value(&self) -> Result<std::sync::MutexGuard<'_, Option<Vec<u8>>>, SecretStoreError> {
            self.value
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)
        }
    }
    impl SecretStore for Secrets {
        fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(self
                .value()?
                .as_ref()
                .map(|value| SecretValue::new(value.clone())))
        }
        fn store(&self, _: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
            *self.value()? = Some(secret.expose().to_vec());
            if self.fail_store_after_write {
                Err(SecretStoreError::OperationFailed)
            } else {
                Ok(())
            }
        }
        fn delete(&self, _: &CredentialId) -> Result<(), SecretStoreError> {
            if self.fail_delete {
                return Err(SecretStoreError::OperationFailed);
            }
            *self.value()? = None;
            Ok(())
        }
    }

    struct Probe {
        succeeds: bool,
    }
    impl SshCapabilityProbePort for Probe {
        fn probe(&self, _: &VdsProfile) -> Result<SshCaptureCapabilities, SshCapabilityProbeError> {
            self.succeeds
                .then_some(SshCaptureCapabilities { tar_zstd: true })
                .ok_or(SshCapabilityProbeError::Unavailable)
        }
    }
}
