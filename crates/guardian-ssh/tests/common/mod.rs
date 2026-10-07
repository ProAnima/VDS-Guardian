// Each test binary uses only part of this shared module.
#![allow(dead_code)]

//! Identities for argument-construction tests: nothing here touches a real key or server.

use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use guardian_ssh::SshIdentity;
use std::path::PathBuf;

pub const TEST_PASSWORD: &str = "S3cret-Pass!";

struct FixedSecret(Vec<u8>);

impl SecretStore for FixedSecret {
    fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(Some(SecretValue::new(self.0.clone())))
    }

    fn store(&self, _: &CredentialId, _: &SecretValue) -> Result<(), SecretStoreError> {
        Ok(())
    }

    fn delete(&self, _: &CredentialId) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

/// A private-key identity backed by a structurally valid (but useless) PEM envelope.
pub fn key_identity() -> Result<SshIdentity, Box<dyn std::error::Error>> {
    let pem = "-----BEGIN EC PRIVATE KEY-----\nMAkCAQACBAECAwQ=\n-----END EC PRIVATE KEY-----\n";
    Ok(SshIdentity::from_store(
        &FixedSecret(pem.as_bytes().to_vec()),
        &CredentialId::parse("credential-test")?,
    )?)
}

/// A password identity that names a helper path but never runs it.
pub fn password_identity() -> Result<SshIdentity, Box<dyn std::error::Error>> {
    Ok(SshIdentity::password_with_helper(
        TEST_PASSWORD.as_bytes(),
        PathBuf::from("C:/helper/guardian-askpass.exe"),
    )?)
}
