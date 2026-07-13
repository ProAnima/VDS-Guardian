use guardian_core::{
    BackupId, Manifest, PayloadEntry, PayloadPath, PlanId, PlanReference, Producer, ProfileId,
    RunId, SourceIdentity, Timestamp,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/manifest-v1.json");

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
