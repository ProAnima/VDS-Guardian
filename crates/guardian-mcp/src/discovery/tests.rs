use super::SshProfileSummary;
use guardian_core::{AuthKind, CredentialId, HostPin, ProfileId, SshEndpoint, VdsProfile};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn profile(auth_kind: Option<AuthKind>) -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-a")?,
        label: "Production".to_owned(),
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: HostPin::parse(
                "ssh-ed25519",
                "AAAAC3NzaC1lZDI1NTE5AAAAIAgqcoUnCQ7+eoqxrmBwksjYpoBztPS3lYA3OqOZvZ+K",
            )?,
        },
        credential_id: CredentialId::parse("credential-a")?,
        auth_kind,
    })
}

#[test]
fn a_profile_summary_reports_the_recorded_login_kind_like_the_desktop() -> TestResult {
    for (kind, name) in [
        (AuthKind::SshKey, "ssh_key"),
        (AuthKind::SshAgent, "ssh_agent"),
        (AuthKind::Password, "password"),
    ] {
        let summary = serde_json::to_value(SshProfileSummary::from(&profile(Some(kind))?))?;
        assert_eq!(summary["authKind"], name);
        assert_eq!(summary["profileId"], "profile-a");
    }
    Ok(())
}

#[test]
fn an_unrecorded_login_kind_is_omitted_rather_than_guessed() -> TestResult {
    let summary = SshProfileSummary::from(&profile(None)?);
    assert_eq!(summary.auth_kind, None);
    let rendered = serde_json::to_value(&summary)?;
    assert!(rendered.get("authKind").is_none(), "{rendered}");
    assert!(
        !rendered.to_string().contains("credential-a"),
        "no credential reference is exposed"
    );
    Ok(())
}
