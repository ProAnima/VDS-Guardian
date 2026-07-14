use guardian_core::{CredentialId, ProfileId, SshEndpoint, VdsProfile};

#[test]
fn profile_requires_a_pinned_non_secret_ssh_endpoint() -> Result<(), Box<dyn std::error::Error>> {
    let profile = VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "Production VDS".to_owned(),
        credential_id: CredentialId::parse("credential-001")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_key_algorithm: "ssh-ed25519".to_owned(),
            host_key_base64: "AAAAC3NzaC1lZDI1NTE5AAAAIexample".to_owned(),
        },
    };
    profile.validate()?;
    Ok(())
}

#[test]
fn profile_rejects_injection_and_unpinned_endpoints() -> Result<(), Box<dyn std::error::Error>> {
    let mut endpoint = SshEndpoint {
        host: "vds.example".to_owned(),
        port: 22,
        user: "backup".to_owned(),
        host_key_algorithm: "ssh-ed25519".to_owned(),
        host_key_base64: "key".to_owned(),
    };
    endpoint.user = "backup;whoami".to_owned();
    assert!(endpoint.validate().is_err());
    endpoint.user = "backup".to_owned();
    endpoint.host_key_base64.clear();
    assert!(endpoint.validate().is_err());
    Ok(())
}
