#![allow(dead_code)]

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use guardian_core::{
    BackupId, Manifest, ManifestSigner, PlanId, PlanReference, Producer, ProfileId, RepositoryId,
    RunId, SigningError, SourceIdentity, Timestamp,
};
use guardian_local_repository::LocalRepository;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn repository(root: &TestRoot) -> Result<LocalRepository, Box<dyn std::error::Error>> {
    Ok(LocalRepository::open(
        root.path(),
        RepositoryId::parse("repository-test")?,
    )?)
}

pub fn manifest(backup_id: &str, run_id: RunId) -> Result<Manifest, Box<dyn std::error::Error>> {
    Ok(Manifest::new(
        BackupId::parse(backup_id)?,
        run_id,
        timestamp("2026-07-13T12:00:00Z")?,
        Producer {
            name: "VDS Guardian test source".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "test".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-test")?,
            host_key_fingerprint: "SHA256:test-fixture".to_owned(),
        },
        PlanReference {
            plan_id: PlanId::parse("plan-test")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    ))
}

pub fn timestamp(value: &str) -> Result<Timestamp, guardian_core::IdentifierError> {
    Timestamp::parse(value)
}

pub struct TestSigner {
    key: SigningKey,
}

impl TestSigner {
    pub fn new() -> Self {
        Self {
            key: SigningKey::from_bytes(&[7_u8; 32]),
        }
    }
}

impl ManifestSigner for TestSigner {
    fn algorithm(&self) -> &'static str {
        "Ed25519"
    }

    fn key_id(&self) -> &str {
        "test-ed25519-key"
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        Ok(self.key.sign(message).to_bytes().to_vec())
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError> {
        let signature =
            Signature::from_slice(signature).map_err(|_| SigningError::VerificationFailed)?;
        self.key
            .verifying_key()
            .verify(message, &signature)
            .map_err(|_| SigningError::VerificationFailed)
    }
}

pub struct RejectingSigner(pub TestSigner);

impl ManifestSigner for RejectingSigner {
    fn algorithm(&self) -> &'static str {
        self.0.algorithm()
    }

    fn key_id(&self) -> &str {
        self.0.key_id()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        self.0.sign(message)
    }

    fn verify(&self, _message: &[u8], _signature: &[u8]) -> Result<(), SigningError> {
        Err(SigningError::VerificationFailed)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSignature {
    algorithm: String,
    key_id: String,
    signature: String,
}

pub fn verify_stored_signature(path: &Path, signer: &TestSigner) -> TestResult {
    let canonical = fs::read(path.join("manifest.json"))?;
    let stored: StoredSignature = serde_json::from_slice(&fs::read(path.join("manifest.sig"))?)?;
    assert_eq!(stored.algorithm, signer.algorithm());
    assert_eq!(stored.key_id, signer.key_id());
    signer.verify(&canonical, &decode_hex(&stored.signature)?)?;
    Ok(())
}

fn decode_hex(value: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !value.len().is_multiple_of(2) {
        return Err("hex length must be even".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(text, 16)?)
        })
        .collect()
}

pub struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    pub fn new() -> Result<Self, std::io::Error> {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vds-guardian-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
