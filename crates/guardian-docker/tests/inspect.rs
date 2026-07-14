use guardian_docker::parse_inspect_json;

#[test]
fn parser_extracts_recovery_relevant_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let inventory = parse_inspect_json(br#"[
      {
        "Id":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "Name":"/postgres",
        "Image":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "Config":{"Image":"postgres:16.4","Labels":{"com.docker.compose.project":"production"}},
        "State":{"Status":"running","Health":{"Status":"healthy"}},
        "Mounts":[
          {"Type":"volume","Name":"postgres-data","Destination":"/var/lib/postgresql/data","RW":true},
          {"Type":"bind","Source":"/srv/secrets/db-password","Destination":"/run/secrets/db_password","RW":false}
        ],
        "NetworkSettings":{"Networks":{"backend":{"NetworkID":"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"}}}
      }
    ]"#)?;
    let container = &inventory.containers[0];
    assert_eq!(container.compose_project.as_deref(), Some("production"));
    assert_eq!(container.secret_references, ["db_password"]);
    assert_eq!(container.networks[0].name, "backend");
    Ok(())
}

#[test]
fn parser_rejects_untrusted_status_and_mount_targets() {
    let unsupported_status = br#"[{"Id":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Name":"/db","Image":"","Config":{"Image":"postgres","Labels":{}},"State":{"Status":"unknown"},"Mounts":[],"NetworkSettings":{"Networks":{}}}]"#;
    assert!(parse_inspect_json(unsupported_status).is_err());

    let traversal_mount = br#"[{"Id":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Name":"/db","Image":"","Config":{"Image":"postgres","Labels":{}},"State":{"Status":"running"},"Mounts":[{"Type":"volume","Name":"data","Destination":"/var/../escape","RW":true}],"NetworkSettings":{"Networks":{}}}]"#;
    assert!(parse_inspect_json(traversal_mount).is_err());
}
