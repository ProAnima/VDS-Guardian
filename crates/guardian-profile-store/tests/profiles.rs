use guardian_core::{
    CredentialId, HostPin, ProfileId, SecretStore, SecretStoreError, SecretValue, SshEndpoint,
    VdsProfile,
};
use guardian_profile_store::{ProfileDeletionError, ProfileStore};
use std::{fs, sync::Mutex};

#[test]
fn profiles_round_trip_and_preserve_safe_future_document_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = ProfileStore::at(root.path());
    store.upsert(profile()?)?;
    assert_eq!(store.list()?.len(), 1);
    fs::write(
        root.path().join("profiles.json"),
        br#"{"formatVersion":1,"profiles":{},"unknown":true}"#,
    )?;
    assert!(store.list().is_ok());
    store.upsert(profile()?)?;
    assert!(fs::read_to_string(root.path().join("profiles.json"))?.contains("\"unknown\":true"));
    Ok(())
}

#[test]
fn deletion_removes_profile_and_credential() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = ProfileStore::at(root.path());
    let profile = profile()?;
    let credential_id = profile.credential_id.clone();
    store.upsert(profile)?;
    let secrets = RecordingSecrets::default();

    assert!(store.remove_with_secret(&ProfileId::parse("profile-001")?, &secrets)?);
    assert!(store.list()?.is_empty());
    assert_eq!(secrets.deleted.into_inner()?, vec![credential_id]);
    Ok(())
}

#[test]
fn failed_credential_cleanup_restores_profile() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = ProfileStore::at(root.path());
    store.upsert(profile()?)?;
    let secrets = RecordingSecrets {
        fail_delete: true,
        ..RecordingSecrets::default()
    };

    assert!(matches!(
        store.remove_with_secret(&ProfileId::parse("profile-001")?, &secrets),
        Err(ProfileDeletionError::Secret(SecretStoreError::AccessDenied))
    ));
    assert_eq!(store.list()?.len(), 1);
    Ok(())
}

#[derive(Default)]
struct RecordingSecrets {
    deleted: Mutex<Vec<CredentialId>>,
    fail_delete: bool,
}

impl SecretStore for RecordingSecrets {
    fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(None)
    }
    fn store(&self, _: &CredentialId, _: &SecretValue) -> Result<(), SecretStoreError> {
        Ok(())
    }
    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        if self.fail_delete {
            return Err(SecretStoreError::AccessDenied);
        }
        self.deleted
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?
            .push(id.clone());
        Ok(())
    }
}

fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "VDS".to_owned(),
        credential_id: CredentialId::parse("credential-001")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: HostPin::parse("ssh-ed25519", "AAAAC3NzaC1lZDI1NTE5AQ==")?,
        },
    })
}
