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
        host_path: None,
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

#[test]
fn inventory_rejects_an_unsafe_host_path() {
    let mut relative_host_path = inventory();
    relative_host_path.containers[0].mounts[0].host_path = Some("relative/path".to_owned());
    assert_eq!(
        relative_host_path.validate(),
        Err(DockerInventoryError::InvalidMount)
    );

    let mut traversal_host_path = inventory();
    traversal_host_path.containers[0].mounts[0].host_path =
        Some("/var/lib/docker/volumes/../../etc".to_owned());
    assert_eq!(
        traversal_host_path.validate(),
        Err(DockerInventoryError::InvalidMount)
    );
}

#[test]
fn capturable_path_resolves_per_mount_kind() {
    let bind = DockerMount {
        kind: DockerMountKind::Bind,
        source_reference: "/srv/secrets/db-password".to_owned(),
        host_path: None,
        destination: "/run/secrets/db_password".to_owned(),
        read_only: true,
    };
    assert_eq!(bind.capturable_path(), Some("/srv/secrets/db-password"));

    let resolved_volume = DockerMount {
        kind: DockerMountKind::Volume,
        source_reference: "postgres-data".to_owned(),
        host_path: Some("/var/lib/docker/volumes/postgres-data/_data".to_owned()),
        destination: "/var/lib/postgresql/data".to_owned(),
        read_only: false,
    };
    assert_eq!(
        resolved_volume.capturable_path(),
        Some("/var/lib/docker/volumes/postgres-data/_data")
    );

    let unresolved_volume = DockerMount {
        kind: DockerMountKind::Volume,
        source_reference: "postgres-data".to_owned(),
        host_path: None,
        destination: "/var/lib/postgresql/data".to_owned(),
        read_only: false,
    };
    assert_eq!(unresolved_volume.capturable_path(), None);

    let tmpfs = DockerMount {
        kind: DockerMountKind::Tmpfs,
        source_reference: "tmpfs".to_owned(),
        host_path: None,
        destination: "/tmp/cache".to_owned(),
        read_only: false,
    };
    assert_eq!(tmpfs.capturable_path(), None);
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
                host_path: Some("/var/lib/docker/volumes/postgres-data/_data".to_owned()),
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
