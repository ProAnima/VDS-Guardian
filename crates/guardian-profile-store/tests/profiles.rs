use guardian_core::{CredentialId, HostPin, ProfileId, SshEndpoint, VdsProfile};
use guardian_profile_store::ProfileStore;
use std::fs;

#[test]
fn profiles_round_trip_and_unknown_fields_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = ProfileStore::at(root.path());
    store.upsert(profile()?)?;
    assert_eq!(store.list()?.len(), 1);
    fs::write(
        root.path().join("profiles.json"),
        br#"{"formatVersion":1,"profiles":{},"unknown":true}"#,
    )?;
    assert!(store.list().is_err());
    Ok(())
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
