use guardian_core::{
    BackupSelection, BackupSelectionItem, CaptureSelectionError, CaptureSelectionWarning,
    DockerContainer, DockerContainerState, DockerHealth, DockerInventory, DockerMount,
    DockerMountKind, ProfileId, RemotePath, RepositoryId, preview_capture_selection,
};

#[test]
fn preview_normalizes_nested_paths_and_warns_about_live_docker_data()
-> Result<(), Box<dyn std::error::Error>> {
    let selection = BackupSelection {
        profile_id: ProfileId::parse("profile-main")?,
        repository_id: RepositoryId::parse("repository-main")?,
        items: vec![
            BackupSelectionItem::RemotePath {
                absolute_path: RemotePath::parse("/srv")?,
            },
            mount_item("/srv/app/data")?,
        ],
        sqlite_path: Some(RemotePath::parse("/srv/app/app.sqlite")?),
    };
    let preview = preview_capture_selection(&selection, Some(&inventory()))?;
    assert_eq!(preview.normalized_roots, vec![RemotePath::parse("/srv")?]);
    assert!(preview.warnings.iter().any(|warning| matches!(warning, CaptureSelectionWarning::CoveredPath { path, .. } if path.as_str() == "/srv/app/data")));
    assert!(preview.warnings.iter().any(|warning| matches!(warning, CaptureSelectionWarning::LiveDockerData { container_id, .. } if container_id == "0123456789ab")));
    assert!(preview.warnings.iter().any(|warning| matches!(
        warning,
        CaptureSelectionWarning::SqliteAlsoInFilesystem { .. }
    )));
    assert!(
        preview
            .confirmation
            .starts_with("CREATE BACKUP FOR profile-main IN repository-main ")
    );
    assert_eq!(
        preview.source_layout.roots,
        vec![RemotePath::parse("/srv")?]
    );
    assert_eq!(preview.source_layout.docker_workloads.len(), 1);
    assert_eq!(
        preview.source_layout.docker_workloads[0].container_name,
        "demo-web"
    );
    assert_eq!(
        preview.source_layout.docker_workloads[0].mounts[0].source_path,
        RemotePath::parse("/srv/app/data")?
    );
    Ok(())
}

#[test]
fn preview_rejects_client_supplied_docker_path_that_changed()
-> Result<(), Box<dyn std::error::Error>> {
    let selection = BackupSelection {
        profile_id: ProfileId::parse("profile-main")?,
        repository_id: RepositoryId::parse("repository-main")?,
        items: vec![mount_item("/etc")?],
        sqlite_path: None,
    };
    assert_eq!(
        preview_capture_selection(&selection, Some(&inventory())),
        Err(CaptureSelectionError::DockerSelectionChanged),
    );
    Ok(())
}

#[test]
fn group_selection_requires_the_complete_current_mount_set()
-> Result<(), Box<dyn std::error::Error>> {
    let selection = BackupSelection {
        profile_id: ProfileId::parse("profile-main")?,
        repository_id: RepositoryId::parse("repository-main")?,
        items: vec![BackupSelectionItem::DockerGroup {
            group_id: "demo".to_owned(),
            capturable_paths: vec![RemotePath::parse("/srv/app/data")?],
        }],
        sqlite_path: None,
    };
    assert!(preview_capture_selection(&selection, Some(&inventory())).is_ok());
    let mut stale = selection;
    stale.items = vec![BackupSelectionItem::DockerGroup {
        group_id: "demo".to_owned(),
        capturable_paths: vec![RemotePath::parse("/srv/app/other")?],
    }];
    assert_eq!(
        preview_capture_selection(&stale, Some(&inventory())),
        Err(CaptureSelectionError::DockerSelectionChanged),
    );
    Ok(())
}

fn mount_item(path: &str) -> Result<BackupSelectionItem, Box<dyn std::error::Error>> {
    Ok(BackupSelectionItem::DockerMount {
        container_id: "0123456789ab".to_owned(),
        mount_destination: RemotePath::parse("/var/lib/app")?,
        capturable_path: RemotePath::parse(path)?,
    })
}

fn inventory() -> DockerInventory {
    DockerInventory {
        containers: vec![DockerContainer {
            id: "0123456789ab".to_owned(),
            name: "demo-web".to_owned(),
            image: "demo:latest".to_owned(),
            image_digest: None,
            compose_project: Some("demo".to_owned()),
            state: DockerContainerState::Running,
            health: Some(DockerHealth::Healthy),
            mounts: vec![DockerMount {
                kind: DockerMountKind::Bind,
                source_reference: "/srv/app/data".to_owned(),
                host_path: None,
                destination: "/var/lib/app".to_owned(),
                read_only: false,
            }],
            networks: vec![],
            secret_references: vec![],
        }],
    }
}
