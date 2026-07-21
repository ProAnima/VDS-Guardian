use base64::Engine as _;
use guardian_core::{
    BackupId, CredentialId, DeploymentPlan, DeploymentPlanError, HostPin, Manifest, PayloadEntry,
    PayloadPath, PlanReference, Producer, ProfileId, RemoteTargetPath, SourceIdentity, SshEndpoint,
    Timestamp, VdsProfile,
};

#[test]
fn deploy_plan_requires_a_sealed_backup_and_exact_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest_with_filesystem_payload()?;
    let target = profile("profile-target", "target-001")?;
    let target_path = RemoteTargetPath::parse("/srv/app")?;
    let plan = DeploymentPlan::build(&manifest, &target, target_path)?;
    assert_eq!(
        plan.filesystem_payload.as_str(),
        "payload/filesystem.tar.zst"
    );
    assert!(plan.approve("DEPLOY something else").is_err());
    assert!(plan.approve(&plan.confirmation).is_ok());
    Ok(())
}

#[test]
fn deploy_plan_confirmation_embeds_identifiers_not_the_label()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest_with_filesystem_payload()?;
    let target = profile("profile-target", "target-001")?;
    let target_path = RemoteTargetPath::parse("/srv/app")?;
    let plan = DeploymentPlan::build(&manifest, &target, target_path)?;
    assert!(plan.confirmation.contains("profile-target"));
    assert!(plan.confirmation.contains("/srv/app"));
    assert!(!plan.confirmation.contains(&target.label));
    Ok(())
}

#[test]
fn deploy_plan_allows_a_new_path_on_the_source_profile() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest_with_filesystem_payload()?;
    let target = profile("profile-source", "target-002")?;
    let target_path = RemoteTargetPath::parse("/srv/app")?;
    assert!(DeploymentPlan::build(&manifest, &target, target_path).is_ok());
    Ok(())
}

#[test]
fn deploy_plan_allows_a_new_path_on_a_reenrolled_source_host()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest_with_filesystem_payload()?;
    // A differently-named profile, but pinned to the exact same host key as
    // the manifest's recorded source -- the more realistic self-overwrite
    // accident (re-enrolling the same physical box under a second label).
    let target = VdsProfile {
        profile_id: ProfileId::parse("profile-different-name")?,
        label: "Re-enrolled source".to_owned(),
        credential_id: CredentialId::parse("credential-002")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: pin(0)?, // same marker as the source's pin -> identical fingerprint
        },
    };
    let target_path = RemoteTargetPath::parse("/srv/app")?;
    assert!(DeploymentPlan::build(&manifest, &target, target_path).is_ok());
    Ok(())
}

#[test]
fn deploy_plan_rejects_a_backup_with_no_filesystem_payload()
-> Result<(), Box<dyn std::error::Error>> {
    // A manifest needs at least one payload to be sealable at all (`prepare_for_seal`
    // rejects `EmptyPayload`), so this exercises the "has payloads, but none of
    // them are the filesystem kind" branch specifically -- a database-only backup.
    let mut manifest = manifest_without_payloads()?;
    manifest.add_payload(PayloadEntry {
        logical_role: "database".to_owned(),
        path: PayloadPath::parse("payload/database.sqlite.zst")?,
        byte_length: 1,
        sha256: "a".repeat(64),
        media_type: "application/vnd.sqlite3+zstd".to_owned(),
        encryption: None,
    })?;
    manifest.prepare_for_seal(
        Timestamp::parse("2026-07-14T20:00:00Z")?,
        "Ed25519",
        &format!("ed25519:{}", "b".repeat(64)),
    )?;
    let target = profile("profile-target", "target-003")?;
    let target_path = RemoteTargetPath::parse("/srv/app")?;
    assert!(matches!(
        DeploymentPlan::build(&manifest, &target, target_path),
        Err(DeploymentPlanError::NoFilesystemPayload)
    ));
    Ok(())
}

fn sealed_manifest_with_filesystem_payload() -> Result<Manifest, Box<dyn std::error::Error>> {
    let mut manifest = manifest_without_payloads()?;
    manifest.add_payload(PayloadEntry {
        logical_role: "filesystem".to_owned(),
        path: PayloadPath::parse("payload/filesystem.tar.zst")?,
        byte_length: 1,
        sha256: "a".repeat(64),
        media_type: "application/zstd".to_owned(),
        encryption: None,
    })?;
    manifest.prepare_for_seal(
        Timestamp::parse("2026-07-14T20:00:00Z")?,
        "Ed25519",
        &format!("ed25519:{}", "b".repeat(64)),
    )?;
    Ok(manifest)
}

fn manifest_without_payloads() -> Result<Manifest, Box<dyn std::error::Error>> {
    Ok(Manifest::new(
        BackupId::parse("backup-001")?,
        guardian_core::RunId::parse("run-001")?,
        Timestamp::parse("2026-07-14T19:00:00Z")?,
        Producer {
            name: "VDS Guardian".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "windows".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-source")?,
            host_key_fingerprint: guardian_core::host_key_fingerprint(
                pin(0)?.public_key_base64.as_str(),
            ),
        },
        PlanReference {
            plan_id: guardian_core::PlanId::parse("plan-001")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    ))
}

fn profile(
    profile_id: &str,
    credential_id: &str,
) -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse(profile_id)?,
        label: "Target VDS".to_owned(),
        credential_id: CredentialId::parse(credential_id)?,
        endpoint: SshEndpoint {
            host: "target.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: pin(2)?,
        },
    })
}

fn pin(marker: u8) -> Result<HostPin, Box<dyn std::error::Error>> {
    let mut blob = Vec::new();
    blob.extend_from_slice(&11_u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.extend_from_slice(&[marker]);
    Ok(HostPin::parse(
        "ssh-ed25519",
        base64::engine::general_purpose::STANDARD.encode(blob),
    )?)
}
