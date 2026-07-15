use fs2::FileExt;
use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use guardian_vault::{EncryptedFileVault, VaultError, VaultFailure, VaultInitOutcome, VaultState};
use std::fs::OpenOptions;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn stored_secrets_round_trip() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-001")?;
    vault.store(&id, &SecretValue::new(b"top-secret".to_vec()))?;
    let loaded = vault.load(&id)?.ok_or("missing secret")?;
    assert_eq!(loaded.expose(), b"top-secret");
    Ok(())
}

#[test]
fn unknown_credential_loads_as_none() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-missing")?;
    assert!(vault.load(&id)?.is_none());
    Ok(())
}

#[test]
fn deleting_an_absent_credential_is_not_an_error() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-absent")?;
    vault.delete(&id)?;
    Ok(())
}

#[test]
fn storing_twice_overwrites_the_previous_secret() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-overwrite")?;
    vault.store(&id, &SecretValue::new(b"first".to_vec()))?;
    vault.store(&id, &SecretValue::new(b"second".to_vec()))?;
    let loaded = vault.load(&id)?.ok_or("missing secret")?;
    assert_eq!(loaded.expose(), b"second");
    Ok(())
}

#[test]
fn a_secret_over_the_size_limit_is_rejected() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-oversized")?;
    let oversized = vec![0_u8; 64 * 1024 + 1];
    assert_eq!(
        vault.store(&id, &SecretValue::new(oversized)),
        Err(SecretStoreError::InvalidData)
    );
    assert!(vault.load(&id)?.is_none());
    Ok(())
}

#[test]
fn open_on_an_uninitialized_directory_fails_closed_and_creates_nothing() -> TestResult {
    let root = tempfile::tempdir()?;
    let vault_dir = root.path().join("vault");
    assert!(matches!(
        EncryptedFileVault::open(&vault_dir),
        Err(VaultError::NotInitialized)
    ));
    assert!(!vault_dir.exists());
    Ok(())
}

#[test]
fn initializing_twice_fails_closed_without_touching_the_existing_key() -> TestResult {
    let root = tempfile::tempdir()?;
    assert!(matches!(
        EncryptedFileVault::init(root.path())?,
        VaultInitOutcome::Created
    ));
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-before-reinit")?;
    vault.store(&id, &SecretValue::new(b"still-here".to_vec()))?;

    assert!(matches!(
        EncryptedFileVault::init(root.path()),
        Err(VaultError::AlreadyInitialized)
    ));

    let reopened = EncryptedFileVault::open(root.path())?;
    let loaded = reopened.load(&id)?.ok_or("missing secret")?;
    assert_eq!(loaded.expose(), b"still-here");
    Ok(())
}

#[test]
fn a_partial_init_missing_only_the_canary_is_recovered_not_regenerated() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-partial-init")?;
    vault.store(&id, &SecretValue::new(b"kept-across-recovery".to_vec()))?;
    std::fs::remove_file(root.path().join("canary.enc"))?;

    assert!(matches!(
        EncryptedFileVault::init(root.path())?,
        VaultInitOutcome::Recovered
    ));

    let reopened = EncryptedFileVault::open(root.path())?;
    let loaded = reopened.load(&id)?.ok_or("missing secret")?;
    assert_eq!(loaded.expose(), b"kept-across-recovery");
    Ok(())
}

#[test]
fn status_never_creates_anything() -> TestResult {
    let root = tempfile::tempdir()?;
    let vault_dir = root.path().join("vault");
    assert_eq!(
        EncryptedFileVault::status(&vault_dir).state,
        VaultState::NotInitialized
    );
    assert!(!vault_dir.exists());

    EncryptedFileVault::init(&vault_dir)?;
    let before = directory_entries(&vault_dir)?;
    assert_eq!(
        EncryptedFileVault::status(&vault_dir).state,
        VaultState::Ready
    );
    let after = directory_entries(&vault_dir)?;
    assert_eq!(before, after);
    Ok(())
}

#[test]
fn a_truncated_master_key_fails_closed_distinctly_from_not_initialized() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    std::fs::write(root.path().join("vault.key"), b"too-short")?;
    assert!(matches!(
        EncryptedFileVault::open(root.path()),
        Err(VaultError::Corrupt)
    ));
    Ok(())
}

#[test]
fn a_tampered_canary_fails_closed() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    flip_last_byte(&root.path().join("canary.enc"))?;
    assert!(matches!(
        EncryptedFileVault::open(root.path()),
        Err(VaultError::Corrupt)
    ));
    assert_eq!(
        EncryptedFileVault::status(root.path()).state,
        VaultState::Corrupt
    );
    Ok(())
}

#[test]
fn swapping_two_credentials_ciphertext_fails_closed() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let a = CredentialId::parse("credential-a")?;
    let b = CredentialId::parse("credential-b")?;
    vault.store(&a, &SecretValue::new(b"secret-a".to_vec()))?;
    vault.store(&b, &SecretValue::new(b"secret-b".to_vec()))?;

    let secrets_dir = root.path().join("secrets");
    let path_a = secrets_dir.join("credential-a.enc");
    let path_b = secrets_dir.join("credential-b.enc");
    let bytes_a = std::fs::read(&path_a)?;
    let bytes_b = std::fs::read(&path_b)?;
    std::fs::write(&path_a, bytes_b)?;
    std::fs::write(&path_b, bytes_a)?;

    assert!(matches!(vault.load(&a), Err(SecretStoreError::InvalidData)));
    assert!(matches!(vault.load(&b), Err(SecretStoreError::InvalidData)));
    Ok(())
}

#[test]
fn a_single_bit_flip_in_a_secret_fails_closed() -> TestResult {
    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let id = CredentialId::parse("credential-tamper")?;
    vault.store(&id, &SecretValue::new(b"tamper-me".to_vec()))?;
    flip_last_byte(&root.path().join("secrets").join("credential-tamper.enc"))?;
    assert!(matches!(
        vault.load(&id),
        Err(SecretStoreError::InvalidData)
    ));
    Ok(())
}

#[test]
fn os_file_lock_rejects_concurrent_init() -> TestResult {
    let root = tempfile::tempdir()?;
    let vault_dir = root.path().join("vault");
    std::fs::create_dir_all(&vault_dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(vault_dir.join("vault.lock"))?;
    lock.try_lock_exclusive()?;

    assert!(matches!(
        EncryptedFileVault::init(&vault_dir),
        Err(VaultError::Busy)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlinked_vault_directory_is_rejected() -> TestResult {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir(&target)?;
    symlink(&target, &link)?;

    assert!(matches!(
        EncryptedFileVault::open(&link),
        Err(VaultError::UnsafeFilesystemEntry)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlinked_secret_file_is_rejected() -> TestResult {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    EncryptedFileVault::init(root.path())?;
    let vault = EncryptedFileVault::open(root.path())?;
    let elsewhere = root.path().join("elsewhere.enc");
    std::fs::write(&elsewhere, b"not a real envelope")?;
    let secret_path = root.path().join("secrets").join("credential-symlinked.enc");
    symlink(&elsewhere, &secret_path)?;

    let id = CredentialId::parse("credential-symlinked")?;
    assert!(matches!(
        vault.load(&id),
        Err(SecretStoreError::OperationFailed)
    ));
    Ok(())
}

#[test]
fn a_vault_failure_never_serializes_the_vault_directory_path() -> TestResult {
    let root = tempfile::tempdir()?;
    let vault_dir = root.path().join("some-vault");
    let Err(error) = EncryptedFileVault::open(&vault_dir) else {
        return Err("expected open to fail".into());
    };
    let failure = VaultFailure::from(error);
    let serialized = serde_json::to_string(&failure)?;
    assert!(!serialized.contains(&vault_dir.to_string_lossy().to_string()));
    Ok(())
}

fn flip_last_byte(path: &Path) -> TestResult {
    let mut bytes = std::fs::read(path)?;
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn directory_entries(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut entries = std::fs::read_dir(path)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    entries.sort();
    Ok(entries)
}
