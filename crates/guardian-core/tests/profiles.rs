use base64::Engine as _;
use guardian_core::{CredentialId, HostPin, ProfileId, SshEndpoint, VdsProfile};

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
            host_pin: pin()?,
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
        host_pin: pin()?,
    };
    endpoint.user = "backup;whoami".to_owned();
    assert!(endpoint.validate().is_err());
    endpoint.user = "backup".to_owned();
    endpoint.host_pin.public_key_base64.clear();
    assert!(endpoint.validate().is_err());
    Ok(())
}

fn pin() -> Result<HostPin, Box<dyn std::error::Error>> {
    let mut blob = Vec::new();
    blob.extend_from_slice(&11_u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.extend_from_slice(&[1]);
    Ok(HostPin::parse(
        "ssh-ed25519",
        base64::engine::general_purpose::STANDARD.encode(blob),
    )?)
}
