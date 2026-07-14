#[cfg(unix)]
use fs2::FileExt;
use guardian_core::{CredentialId, ManifestSigner, SecretStore, SecretStoreError, SecretValue};
use guardian_signing::{
    EnrollmentDisposition, IdentityError, SigningIdentityErrorCode, SigningIdentityFailure,
    SigningIdentityManager, SigningIdentityState,
};
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};

type TestResult = Result<(), Box<dyn std::error::Error>>;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn enrollment_commits_only_a_credential_reference() -> TestResult {
    let root = TestRoot::new()?;
    let store = MemoryStore::default();
    let manager = SigningIdentityManager::open(root.path())?;
    let enrolled = manager.enroll_or_load(&store)?;
    let first_key_id = enrolled.key_id().to_owned();
    let first_credential = enrolled.credential_id().clone();

    assert_eq!(enrolled.disposition(), EnrollmentDisposition::Enrolled);
    assert!(!root.path().join("signing-enrollment.json").exists());
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("signing.json"))?)?;
    assert_eq!(config.as_object().map(serde_json::Map::len), Some(4));
    assert_eq!(config["credentialId"], first_credential.as_str());
    assert_eq!(config["keyId"], first_key_id);

    let loaded = manager.enroll_or_load(&store)?;
    assert_eq!(loaded.disposition(), EnrollmentDisposition::Loaded);
    assert_eq!(loaded.credential_id(), &first_credential);
    assert_eq!(loaded.key_id(), first_key_id);
    let status = manager.status(&store)?;
    assert_eq!(status.state, SigningIdentityState::Ready);
    assert_eq!(status.identity, Some(loaded.descriptor()));
    Ok(())
}

#[test]
fn status_never_starts_enrollment() -> TestResult {
    let root = TestRoot::new()?;
    let store = MemoryStore::default();
    let manager = SigningIdentityManager::open(root.path())?;

    let status = manager.status(&store)?;
    assert_eq!(status.state, SigningIdentityState::NotEnrolled);
    assert!(status.identity.is_none());
    assert!(!root.path().join("signing.json").exists());
    assert!(!root.path().join("signing-enrollment.json").exists());
    assert!(
        store
            .values
            .lock()
            .map_err(|_| "poisoned test store")?
            .is_empty()
    );
    Ok(())
}

#[test]
fn interrupted_keyring_commit_is_recovered_without_rotation() -> TestResult {
    let root = TestRoot::new()?;
    let store = FailReadbackOnceStore::default();
    let manager = SigningIdentityManager::open(root.path())?;

    assert!(matches!(
        manager.enroll_or_load(&store),
        Err(IdentityError::Store(SecretStoreError::OperationFailed))
    ));
    assert!(root.path().join("signing-enrollment.json").is_file());
    assert!(!root.path().join("signing.json").exists());

    let recovered = manager.enroll_or_load(&store)?;
    assert_eq!(recovered.disposition(), EnrollmentDisposition::Recovered);
    assert!(root.path().join("signing.json").is_file());
    assert!(!root.path().join("signing-enrollment.json").exists());
    let signature = recovered.sign(b"recovered manifest")?;
    recovered.verify(b"recovered manifest", &signature)?;
    Ok(())
}

#[test]
fn configuration_tampering_fails_closed() -> TestResult {
    let root = TestRoot::new()?;
    let store = MemoryStore::default();
    let manager = SigningIdentityManager::open(root.path())?;
    manager.enroll_or_load(&store)?;
    let path = root.path().join("signing.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    config["keyId"] = serde_json::Value::String(format!("ed25519:{}", "0".repeat(64)));
    fs::write(path, serde_json::to_vec(&config)?)?;

    assert!(matches!(
        manager.enroll_or_load(&store),
        Err(IdentityError::ConfigurationMismatch)
    ));
    Ok(())
}

#[test]
fn unknown_configuration_fields_fail_closed() -> TestResult {
    let root = TestRoot::new()?;
    let store = MemoryStore::default();
    let manager = SigningIdentityManager::open(root.path())?;
    manager.enroll_or_load(&store)?;
    let path = root.path().join("signing.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    config["privateKey"] = serde_json::Value::String("forbidden".to_owned());
    fs::write(path, serde_json::to_vec(&config)?)?;

    assert!(matches!(
        manager.enroll_or_load(&store),
        Err(IdentityError::IncompatibleConfiguration)
    ));
    Ok(())
}

#[test]
fn public_failures_redact_internal_io_payloads() -> TestResult {
    let internal = IdentityError::Io {
        operation: "read signing metadata",
        source: std::io::Error::other("C:/secret/operator/path"),
    };
    let public = SigningIdentityFailure::from(internal);
    let json = serde_json::to_string(&public)?;

    assert_eq!(public.code, SigningIdentityErrorCode::LocalIoFailure);
    assert!(!json.contains("secret/operator"));
    assert!(!json.contains("read signing metadata"));
    Ok(())
}

#[test]
fn missing_committed_secret_never_rotates_implicitly() -> TestResult {
    let root = TestRoot::new()?;
    let populated = MemoryStore::default();
    let manager = SigningIdentityManager::open(root.path())?;
    manager.enroll_or_load(&populated)?;

    assert!(matches!(
        manager.enroll_or_load(&MemoryStore::default()),
        Err(IdentityError::Missing)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn os_file_lock_rejects_concurrent_enrollment() -> TestResult {
    let root = TestRoot::new()?;
    let manager = SigningIdentityManager::open(root.path())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.path().join("signing.lock"))?;
    lock.try_lock_exclusive()?;

    assert!(matches!(
        manager.enroll_or_load(&MemoryStore::default()),
        Err(IdentityError::Busy)
    ));
    Ok(())
}

#[test]
fn process_lock_rejects_concurrent_enrollment() -> TestResult {
    let root = TestRoot::new()?;
    let store = Arc::new(BlockingStore::default());
    let worker_store = Arc::clone(&store);
    let worker_root = root.path().to_owned();
    let worker = std::thread::spawn(move || {
        let manager = SigningIdentityManager::open(worker_root)?;
        manager.enroll_or_load(worker_store.as_ref()).map(|_| ())
    });
    store.entered.wait();

    let concurrent = SigningIdentityManager::open(root.path())?;
    assert!(matches!(
        concurrent.status(store.as_ref()),
        Err(IdentityError::Busy)
    ));
    store.release.wait();
    worker
        .join()
        .map_err(|_| std::io::Error::other("enrollment worker panicked"))??;
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_configuration_root_is_rejected() -> TestResult {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new()?;
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target)?;
    symlink(&target, &link)?;

    assert!(matches!(
        SigningIdentityManager::open(&link),
        Err(IdentityError::UnsafeFilesystemEntry)
    ));
    Ok(())
}

#[derive(Default)]
struct MemoryStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl SecretStore for MemoryStore {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        let values = self
            .values
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        Ok(values.get(id.as_str()).cloned().map(SecretValue::new))
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        values.insert(id.as_str().to_owned(), secret.expose().to_vec());
        Ok(())
    }
}

#[derive(Default)]
struct FailReadbackOnceStore {
    state: Mutex<FailState>,
}

struct BlockingStore {
    entered: Barrier,
    release: Barrier,
    blocked: AtomicBool,
    value: Mutex<Option<Vec<u8>>>,
}

impl Default for BlockingStore {
    fn default() -> Self {
        Self {
            entered: Barrier::new(2),
            release: Barrier::new(2),
            blocked: AtomicBool::new(false),
            value: Mutex::new(None),
        }
    }
}

impl SecretStore for BlockingStore {
    fn load(&self, _id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        if !self.blocked.swap(true, Ordering::Relaxed) {
            self.entered.wait();
            self.release.wait();
        }
        let value = self
            .value
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        Ok(value.clone().map(SecretValue::new))
    }

    fn store(&self, _id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        let mut value = self
            .value
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        *value = Some(secret.expose().to_vec());
        Ok(())
    }
}

#[derive(Default)]
struct FailState {
    value: Option<Vec<u8>>,
    fail_next_load: bool,
}

impl SecretStore for FailReadbackOnceStore {
    fn load(&self, _id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        if state.fail_next_load {
            state.fail_next_load = false;
            return Err(SecretStoreError::OperationFailed);
        }
        Ok(state.value.clone().map(SecretValue::new))
    }

    fn store(&self, _id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        state.value = Some(secret.expose().to_vec());
        state.fail_next_load = true;
        Ok(())
    }
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new() -> Result<Self, std::io::Error> {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vds-guardian-signing-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
