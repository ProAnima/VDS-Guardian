use guardian_core::{
    DockerContainer, DockerContainerState, DockerHealth, DockerInventory, DockerInventoryError,
    DockerMount, DockerMountKind, DockerNetwork,
};

#[test]
fn inventory_accepts_container_metadata_needed_for_recovery() {
    assert!(inventory().validate().is_ok());
}

#[test]
fn inventory_rejects_duplicate_mount_targets_and_untrusted_paths() {
    let mut duplicate = inventory();
    duplicate.containers[0].mounts.push(DockerMount {
        kind: DockerMountKind::Volume,
        source_reference: "other-data".to_owned(),
        destination: "/var/lib/postgresql/data".to_owned(),
        read_only: false,
    });
    assert_eq!(
        duplicate.validate(),
        Err(DockerInventoryError::DuplicateField)
    );

    let mut traversal = inventory();
    traversal.containers[0].mounts[0].destination = "/var/lib/../escape".to_owned();
    assert_eq!(
        traversal.validate(),
        Err(DockerInventoryError::InvalidMount)
    );
}

fn inventory() -> DockerInventory {
    DockerInventory {
        containers: vec![DockerContainer {
            id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            name: "postgres".to_owned(),
            image: "postgres:16.4".to_owned(),
            image_digest: Some(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
            ),
            compose_project: Some("production".to_owned()),
            state: DockerContainerState::Running,
            health: Some(DockerHealth::Healthy),
            mounts: vec![DockerMount {
                kind: DockerMountKind::Volume,
                source_reference: "postgres-data".to_owned(),
                destination: "/var/lib/postgresql/data".to_owned(),
                read_only: false,
            }],
            networks: vec![DockerNetwork {
                name: "backend".to_owned(),
                network_id: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    .to_owned(),
            }],
            secret_references: vec!["postgres_password".to_owned()],
        }],
    }
}
