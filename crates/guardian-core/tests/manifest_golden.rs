use guardian_core::{
    BackupId, CredentialId, Manifest, PayloadEncryption, PayloadEntry, PayloadPath, PlanId,
    PlanReference, Producer, ProfileId, RunId, SourceIdentity, Timestamp,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/manifest-v1.json");
const FIXTURE_V2: &[u8] = include_bytes!("fixtures/manifest-v2.json");

#[test]
fn canonical_manifest_v1_matches_golden_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = manifest()?;
    manifest.prepare_for_seal(
        Timestamp::parse("2026-07-13T12:05:00Z")?,
        "Ed25519",
        &format!("ed25519:{}", "b".repeat(64)),
    )?;
    let expected = FIXTURE.strip_suffix(b"\n").unwrap_or(FIXTURE);
    assert_eq!(manifest.canonical_bytes()?, expected);
    Ok(())
}

#[test]
fn golden_manifest_is_supported_and_sealed() -> Result<(), Box<dyn std::error::Error>> {
    let parsed: Manifest = serde_json::from_slice(FIXTURE)?;
    parsed.validate_sealed()?;
    assert_eq!(parsed, serde_json::from_slice(&parsed.canonical_bytes()?)?);
    Ok(())
}

/// Format-v2 golden fixture (ADR 0013): an encrypted payload carrying a
/// recovery-wrapped key. No v2 fixture existed before this — v2 is the
/// mandatory shape for every live capture, so its wire format deserves the
/// identical drift protection the v1 fixture above already has.
#[test]
fn canonical_manifest_v2_matches_golden_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = manifest_v2()?;
    manifest.prepare_for_seal(
        Timestamp::parse("2026-07-16T12:05:00Z")?,
        "Ed25519",
        &format!("ed25519:{}", "d".repeat(64)),
    )?;
    let expected = FIXTURE_V2.strip_suffix(b"\n").unwrap_or(FIXTURE_V2);
    assert_eq!(manifest.canonical_bytes()?, expected);
    Ok(())
}

#[test]
fn golden_manifest_v2_is_supported_sealed_and_recovery_wrapped()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed: Manifest = serde_json::from_slice(FIXTURE_V2)?;
    parsed.validate_sealed()?;
    assert_eq!(parsed, serde_json::from_slice(&parsed.canonical_bytes()?)?);
    assert_eq!(parsed.format_version, 2);
    let encryption = parsed.payloads[0]
        .encryption
        .as_ref()
        .ok_or("payload should be encrypted")?;
    assert!(encryption.recovery_wrapped_key()?.is_some());
    Ok(())
}

fn manifest() -> Result<Manifest, Box<dyn std::error::Error>> {
    let mut manifest = Manifest::new(
        BackupId::parse("backup-golden-001")?,
        RunId::parse("run-golden-001")?,
        Timestamp::parse("2026-07-13T12:00:00Z")?,
        Producer {
            name: "VDS Guardian".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "linux-x86_64".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-golden")?,
            host_key_fingerprint: "SHA256:fixture-host-key".to_owned(),
        },
        PlanReference {
            plan_id: PlanId::parse("plan-golden")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    );
    manifest.add_payload(PayloadEntry::new(
        "filesystem",
        PayloadPath::parse("payload/filesystem-000.tar.zst")?,
        11,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        "application/zstd",
    )?)?;
    Ok(manifest)
}

fn manifest_v2() -> Result<Manifest, Box<dyn std::error::Error>> {
    let mut manifest = Manifest::new(
        BackupId::parse("backup-golden-002")?,
        RunId::parse("run-golden-002")?,
        Timestamp::parse("2026-07-16T12:00:00Z")?,
        Producer {
            name: "VDS Guardian".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "linux-x86_64".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-golden")?,
            host_key_fingerprint: "SHA256:fixture-host-key".to_owned(),
        },
        PlanReference {
            plan_id: PlanId::parse("plan-golden")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    );
    let encryption = PayloadEncryption::new(
        1,
        "AES-256-GCM-CHUNKED",
        CredentialId::parse("payload-golden-002")?,
        &[0_u8; 12],
    )?
    .with_recovery_wrapped_key(&[0_u8; 95])?;
    manifest.add_payload(
        PayloadEntry::new(
            "filesystem",
            PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?,
            123,
            "c".repeat(64).as_str(),
            "application/zstd",
        )?
        .encrypted(encryption)?,
    )?;
    Ok(manifest)
}
