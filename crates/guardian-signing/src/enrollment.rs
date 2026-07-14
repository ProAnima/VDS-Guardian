use crate::filesystem::{
    acquire_lock, atomic_write, ensure_directory, read_optional, remove_regular,
};
use crate::public::{
    EnrollmentDisposition, SigningIdentityDescriptor, SigningIdentityEnrollment,
    SigningIdentityState, SigningIdentityStatus,
};
use crate::{Ed25519Identity, IdentityError};
use guardian_core::{CredentialId, ManifestSigner, SecretStore, SigningError};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_VERSION: u32 = 1;
const ALGORITHM: &str = "Ed25519";

pub struct SigningIdentityManager {
    root: PathBuf,
}

impl SigningIdentityManager {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IdentityError> {
        fs::create_dir_all(path.as_ref()).map_err(|source| {
            IdentityError::io("create signing configuration directory", source)
        })?;
        ensure_directory(path.as_ref())?;
        let root = fs::canonicalize(path.as_ref())
            .map_err(|source| IdentityError::io("canonicalize signing configuration", source))?;
        ensure_directory(&root)?;
        Ok(Self { root })
    }

    pub fn enroll_or_load(
        &self,
        store: &dyn SecretStore,
    ) -> Result<ManagedIdentity, IdentityError> {
        let _lock = acquire_lock(&self.root)?;
        if let Some(config) = read_optional::<SigningConfig>(&self.config_path())? {
            return self.load_committed(store, config);
        }
        let intent = match read_optional::<EnrollmentIntent>(&self.intent_path())? {
            Some(intent) => intent.validate()?,
            None => self.create_intent()?,
        };
        self.finish_enrollment(store, intent)
    }

    pub fn status(&self, store: &dyn SecretStore) -> Result<SigningIdentityStatus, IdentityError> {
        let _lock = acquire_lock(&self.root)?;
        if let Some(config) = read_optional::<SigningConfig>(&self.config_path())? {
            let managed = self.load_committed_read_only(store, config)?;
            return Ok(SigningIdentityStatus {
                state: SigningIdentityState::Ready,
                identity: Some(managed.descriptor()),
            });
        }
        self.pending_status(store)
    }

    fn load_committed(
        &self,
        store: &dyn SecretStore,
        config: SigningConfig,
    ) -> Result<ManagedIdentity, IdentityError> {
        let config = config.validate()?;
        let identity = Ed25519Identity::load(store, &config.credential_id)?;
        if identity.key_id() != config.key_id {
            return Err(IdentityError::ConfigurationMismatch);
        }
        remove_regular(&self.intent_path())?;
        Ok(ManagedIdentity {
            credential_id: config.credential_id,
            disposition: EnrollmentDisposition::Loaded,
            identity,
        })
    }

    fn load_committed_read_only(
        &self,
        store: &dyn SecretStore,
        config: SigningConfig,
    ) -> Result<ManagedIdentity, IdentityError> {
        let config = config.validate()?;
        let identity = Ed25519Identity::load(store, &config.credential_id)?;
        if identity.key_id() != config.key_id {
            return Err(IdentityError::ConfigurationMismatch);
        }
        Ok(ManagedIdentity {
            credential_id: config.credential_id,
            disposition: EnrollmentDisposition::Loaded,
            identity,
        })
    }

    fn pending_status(
        &self,
        store: &dyn SecretStore,
    ) -> Result<SigningIdentityStatus, IdentityError> {
        let Some(intent) = read_optional::<EnrollmentIntent>(&self.intent_path())? else {
            return Ok(SigningIdentityStatus {
                state: SigningIdentityState::NotEnrolled,
                identity: None,
            });
        };
        let intent = intent.validate()?;
        match Ed25519Identity::load(store, &intent.credential_id) {
            Ok(identity) => Ok(SigningIdentityStatus {
                state: SigningIdentityState::RecoveryPending,
                identity: Some(descriptor(&intent.credential_id, &identity)),
            }),
            Err(IdentityError::Missing) => Ok(SigningIdentityStatus {
                state: SigningIdentityState::EnrollmentPending,
                identity: None,
            }),
            Err(error) => Err(error),
        }
    }

    fn create_intent(&self) -> Result<EnrollmentIntent, IdentityError> {
        let mut random = [0_u8; 16];
        OsRng.fill_bytes(&mut random);
        let credential_id = CredentialId::parse(format!("signing-{}", hex(&random)))
            .map_err(|_| IdentityError::Serialization)?;
        let intent = EnrollmentIntent {
            format_version: CONFIG_VERSION,
            credential_id,
        };
        atomic_write(&self.intent_path(), &intent)?;
        Ok(intent)
    }

    fn finish_enrollment(
        &self,
        store: &dyn SecretStore,
        intent: EnrollmentIntent,
    ) -> Result<ManagedIdentity, IdentityError> {
        let (identity, disposition) = match Ed25519Identity::load(store, &intent.credential_id) {
            Ok(identity) => (identity, EnrollmentDisposition::Recovered),
            Err(IdentityError::Missing) => (
                Ed25519Identity::enroll_exclusive(store, &intent.credential_id)?,
                EnrollmentDisposition::Enrolled,
            ),
            Err(error) => return Err(error),
        };
        let config = SigningConfig {
            format_version: CONFIG_VERSION,
            credential_id: intent.credential_id.clone(),
            algorithm: ALGORITHM.to_owned(),
            key_id: identity.key_id().to_owned(),
        };
        atomic_write(&self.config_path(), &config)?;
        remove_regular(&self.intent_path())?;
        Ok(ManagedIdentity {
            credential_id: intent.credential_id,
            disposition,
            identity,
        })
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("signing.json")
    }

    fn intent_path(&self) -> PathBuf {
        self.root.join("signing-enrollment.json")
    }
}

pub struct ManagedIdentity {
    credential_id: CredentialId,
    disposition: EnrollmentDisposition,
    identity: Ed25519Identity,
}

impl ManagedIdentity {
    #[must_use]
    pub fn credential_id(&self) -> &CredentialId {
        &self.credential_id
    }

    #[must_use]
    pub fn disposition(&self) -> EnrollmentDisposition {
        self.disposition
    }

    #[must_use]
    pub fn descriptor(&self) -> SigningIdentityDescriptor {
        descriptor(&self.credential_id, &self.identity)
    }

    #[must_use]
    pub fn enrollment(&self) -> SigningIdentityEnrollment {
        SigningIdentityEnrollment {
            disposition: self.disposition,
            identity: self.descriptor(),
        }
    }
}

impl ManifestSigner for ManagedIdentity {
    fn algorithm(&self) -> &'static str {
        self.identity.algorithm()
    }

    fn key_id(&self) -> &str {
        self.identity.key_id()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        self.identity.sign(message)
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError> {
        self.identity.verify(message, signature)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SigningConfig {
    format_version: u32,
    credential_id: CredentialId,
    algorithm: String,
    key_id: String,
}

impl SigningConfig {
    fn validate(self) -> Result<Self, IdentityError> {
        let key_valid = self.key_id.strip_prefix("ed25519:").is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        });
        if self.format_version != CONFIG_VERSION || self.algorithm != ALGORITHM || !key_valid {
            return Err(IdentityError::IncompatibleConfiguration);
        }
        Ok(self)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollmentIntent {
    format_version: u32,
    credential_id: CredentialId,
}

impl EnrollmentIntent {
    fn validate(self) -> Result<Self, IdentityError> {
        if self.format_version != CONFIG_VERSION {
            return Err(IdentityError::IncompatibleConfiguration);
        }
        Ok(self)
    }
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(ALPHABET[usize::from(byte >> 4)]));
        output.push(char::from(ALPHABET[usize::from(byte & 0x0f)]));
    }
    output
}

fn descriptor(
    credential_id: &CredentialId,
    identity: &Ed25519Identity,
) -> SigningIdentityDescriptor {
    SigningIdentityDescriptor {
        credential_id: credential_id.clone(),
        algorithm: identity.algorithm().to_owned(),
        key_id: identity.key_id().to_owned(),
    }
}
