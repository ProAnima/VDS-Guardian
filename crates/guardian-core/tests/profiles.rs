use base64::Engine as _;
use guardian_core::{AuthKind, CredentialId, HostPin, ProfileId, SshEndpoint, VdsProfile};

#[test]
fn profile_requires_a_pinned_non_secret_ssh_endpoint() -> Result<(), Box<dyn std::error::Error>> {
    let profile = VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "Production VDS".to_owned(),
        auth_kind: None,
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

fn profile_with(auth_kind: Option<AuthKind>) -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "Production VDS".to_owned(),
        auth_kind,
        credential_id: CredentialId::parse("credential-001")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "root".to_owned(),
            host_pin: pin()?,
        },
    })
}

#[test]
fn a_profile_saved_before_auth_kind_existed_still_loads_as_having_none()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = serde_json::to_value(profile_with(None)?)?;
    assert!(
        document.get("authKind").is_none(),
        "an unknown kind is not written"
    );
    let legacy: VdsProfile = serde_json::from_value(document.clone())?;
    assert_eq!(legacy.auth_kind, None);
    document["authKind"] = serde_json::json!("password");
    assert_eq!(
        serde_json::from_value::<VdsProfile>(document)?.auth_kind,
        Some(AuthKind::Password)
    );
    Ok(())
}

#[test]
fn every_auth_kind_round_trips_with_its_stable_name() -> Result<(), Box<dyn std::error::Error>> {
    for (kind, name) in [
        (AuthKind::SshKey, "ssh_key"),
        (AuthKind::SshAgent, "ssh_agent"),
        (AuthKind::Password, "password"),
    ] {
        let document = serde_json::to_value(profile_with(Some(kind))?)?;
        assert_eq!(document["authKind"], name);
        assert_eq!(
            serde_json::from_value::<VdsProfile>(document)?.auth_kind,
            Some(kind)
        );
    }
    let unknown = serde_json::json!({"authKind": "telepathy"});
    assert!(serde_json::from_value::<AuthKind>(unknown["authKind"].clone()).is_err());
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
