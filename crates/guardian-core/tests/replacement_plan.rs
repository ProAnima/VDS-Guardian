use base64::Engine as _;
use guardian_core::{
    BackupId, CredentialId, DockerContainer, DockerContainerState, DockerHealth, DockerInventory,
    DockerMount, DockerMountKind, DockerMountSnapshot, DockerWorkloadSnapshot, HostPin, Manifest,
    PayloadEntry, PayloadPath, PlanId, PlanReference, Producer, ProfileId, RemotePath, RunId,
    SourceIdentity, SourceLayout, SourceReplacementPlan, SourceReplacementPlanError, SshEndpoint,
    Timestamp, VdsProfile,
};

#[test]
fn matching_live_inventory_produces_an_executable_state_bound_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest("/srv/app")?;
    let plan = SourceReplacementPlan::build(&manifest, &profile()?)?
        .reconcile_current(Some(&inventory("app:1")))
        .reconcile_source_ready(true);
    assert!(plan.impact.conflicts.is_empty());
    assert_eq!(plan.impact.containers, ["app"]);
    assert!(plan.impact.confirmation.contains(" STATE "));
    assert!(plan.approve(&plan.impact.confirmation).is_ok());
    Ok(())
}

#[test]
fn changed_live_image_is_a_blocking_conflict_and_changes_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest("/srv/app")?;
    let base = SourceReplacementPlan::build(&manifest, &profile()?)?;
    let base_confirmation = base.impact.confirmation.clone();
    let plan = base
        .reconcile_current(Some(&inventory("app:2")))
        .reconcile_source_ready(true);
    assert_eq!(plan.impact.conflicts, ["container_image_changed:app"]);
    assert_ne!(plan.impact.confirmation, base_confirmation);
    assert!(matches!(
        plan.approve(&plan.impact.confirmation),
        Err(SourceReplacementPlanError::LiveConflicts)
    ));
    Ok(())
}

#[test]
fn an_unavailable_source_root_blocks_execution() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest("/srv/app")?;
    let plan = SourceReplacementPlan::build(&manifest, &profile()?)?
        .reconcile_current(Some(&inventory("app:1")))
        .reconcile_source_ready(false);
    assert_eq!(plan.impact.conflicts, ["source_root_unavailable:/srv/app"]);
    assert!(plan.approve(&plan.impact.confirmation).is_err());
    Ok(())
}

#[test]
fn operating_system_roots_cannot_be_replaced() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = sealed_manifest("/etc")?;
    assert!(matches!(
        SourceReplacementPlan::build(&manifest, &profile()?),
        Err(SourceReplacementPlanError::UnsafeRoot)
    ));
    Ok(())
}

fn sealed_manifest(root: &str) -> Result<Manifest, Box<dyn std::error::Error>> {
    let host_pin = pin()?;
    let mut manifest = Manifest::new(
        BackupId::parse("backup-replace")?,
        RunId::parse("run-replace")?,
        Timestamp::parse("2026-07-21T18:00:00Z")?,
        Producer {
            name: "VDS Guardian".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "linux".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-source")?,
            host_key_fingerprint: guardian_core::host_key_fingerprint(&host_pin.public_key_base64),
        },
        PlanReference {
            plan_id: PlanId::parse("plan-replace")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    );
    let source = RemotePath::parse(root)?;
    manifest.source_layout = Some(SourceLayout {
        roots: vec![source.clone()],
        docker_workloads: vec![DockerWorkloadSnapshot {
            container_id: "a".repeat(64),
            container_name: "app".to_owned(),
            image: "app:1".to_owned(),
            image_digest: Some(format!("sha256:{}", "b".repeat(64))),
            compose_project: Some("project".to_owned()),
            state: DockerContainerState::Running,
            mounts: vec![DockerMountSnapshot {
                source_path: source,
                destination: RemotePath::parse("/data")?,
                read_only: false,
            }],
        }],
    });
    manifest.add_payload(PayloadEntry {
        logical_role: "filesystem".to_owned(),
        path: PayloadPath::parse("payload/filesystem.tar.zst")?,
        byte_length: 1,
        sha256: "c".repeat(64),
        media_type: "application/zstd".to_owned(),
        encryption: None,
    })?;
    manifest.prepare_for_seal(
        Timestamp::parse("2026-07-21T18:01:00Z")?,
        "Ed25519",
        &format!("ed25519:{}", "d".repeat(64)),
    )?;
    Ok(manifest)
}

fn inventory(image: &str) -> DockerInventory {
    DockerInventory {
        containers: vec![DockerContainer {
            id: "a".repeat(64),
            name: "app".to_owned(),
            image: image.to_owned(),
            image_digest: Some(format!("sha256:{}", "b".repeat(64))),
            compose_project: Some("project".to_owned()),
            state: DockerContainerState::Running,
            health: Some(DockerHealth::Healthy),
            mounts: vec![DockerMount {
                kind: DockerMountKind::Bind,
                source_reference: "/srv/app".to_owned(),
                host_path: None,
                destination: "/data".to_owned(),
                read_only: false,
            }],
            networks: Vec::new(),
            secret_references: Vec::new(),
        }],
    }
}

fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-source")?,
        label: "Source".to_owned(),
        credential_id: CredentialId::parse("credential-source")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: pin()?,
        },
    })
}

fn pin() -> Result<HostPin, Box<dyn std::error::Error>> {
    let mut blob = Vec::new();
    blob.extend_from_slice(&11_u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.push(7);
    Ok(HostPin::parse(
        "ssh-ed25519",
        base64::engine::general_purpose::STANDARD.encode(blob),
    )?)
}
