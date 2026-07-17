use guardian_core::{
    BackupId, Manifest, PayloadEntry, PayloadPath, PlanReference, Producer, RestorePlan,
    SourceIdentity, Timestamp,
};
use std::path::PathBuf;

#[test]
fn restore_plan_requires_a_sealed_backup_and_exact_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = manifest()?;
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
    let destination = std::env::temp_dir().join("vds-guardian-restore-target");
    let plan = RestorePlan::build(&manifest, &destination)?;
    assert_eq!(
        plan.filesystem_payload.as_str(),
        "payload/filesystem.tar.zst"
    );
    assert!(plan.destination_is_new());
    assert!(plan.approve("RESTORE something else").is_err());
    assert!(plan.approve(&plan.confirmation).is_ok());
    Ok(())
}

#[test]
fn restore_plan_rejects_relative_destination() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = manifest()?;
    assert!(RestorePlan::build(&manifest, PathBuf::from("relative")).is_err());
    Ok(())
}

#[test]
fn restore_impact_lists_adds_and_blocks_an_existing_destination()
-> Result<(), Box<dyn std::error::Error>> {
    let mut manifest = manifest()?;
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
    let destination = std::env::temp_dir().join(guardian_core::RunId::new().as_str());
    std::fs::create_dir(&destination)?;
    let plan = RestorePlan::build(&manifest, &destination)?;
    assert_eq!(plan.impact.adds, vec![destination.clone()]);
    assert!(plan.impact.replaces.is_empty());
    assert_eq!(plan.impact.conflicts, vec![destination.clone()]);
    assert_eq!(plan.impact.workload_labels, vec!["filesystem"]);
    let serialized = serde_json::to_value(&plan.impact)?;
    assert_eq!(serialized["mode"], "new_destination");
    assert_eq!(serialized["replaces"], serde_json::json!([]));
    assert_eq!(
        serialized["workloadLabels"],
        serde_json::json!(["filesystem"])
    );
    assert!(plan.approve(&plan.confirmation).is_err());
    std::fs::remove_dir(destination)?;
    Ok(())
}

fn manifest() -> Result<Manifest, Box<dyn std::error::Error>> {
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
            profile_id: guardian_core::ProfileId::parse("profile-001")?,
            host_key_fingerprint: "SHA256:test".to_owned(),
        },
        PlanReference {
            plan_id: guardian_core::PlanId::parse("plan-001")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    ))
}
