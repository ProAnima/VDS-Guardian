use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_core::{DatabaseAuthentication, DatabaseConnection, DatabaseEngine, DatabaseId};
use guardian_ssh::{PinnedHost, RemoteCapturePlan, SshUser, SystemOpenSsh};
use std::path::Path;

#[test]
fn pinned_capture_uses_only_strict_noninteractive_openssh_options()
-> Result<(), Box<dyn std::error::Error>> {
    let host = pinned_host()?;
    let user = SshUser::parse("backup")?;
    let plan =
        RemoteCapturePlan::from_roots(["/srv/app".to_owned(), "/etc/app config".to_owned()])?;
    let arguments = SystemOpenSsh::default().arguments(
        &host,
        &user,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        &plan,
    );
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("BatchMode=yes"));
    assert!(rendered.contains("ConnectTimeout=30"));
    assert!(rendered.contains("PasswordAuthentication=no"));
    assert!(rendered.contains("PreferredAuthentications=publickey"));
    assert!(rendered.contains("GlobalKnownHostsFile=none"));
    assert!(rendered.contains("backup@vds.example"));
    assert!(rendered.contains("-- '/srv/app' '/etc/app config'"));
    assert!(!rendered.contains("StrictHostKeyChecking=no"));
    Ok(())
}

#[test]
fn capability_probe_is_pinned_noninteractive_and_read_only()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().capability_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
    );
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("ConnectTimeout=30"));
    assert!(rendered.contains("tar --create --zstd --file=/dev/null --files-from=/dev/null"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn connection_probe_is_pinned_and_has_no_operator_command_input()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().connection_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
    );
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.ends_with("\ntrue"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn docker_inventory_command_is_fixed_and_never_accepts_remote_input()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().docker_inspect_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
    );
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("docker ps --all --quiet --no-trunc"));
    assert!(rendered.contains("xargs -r docker inspect --"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn database_tool_probe_uses_only_fixed_read_only_version_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().database_tool_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
    );
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("pg_dump --version"));
    assert!(rendered.contains("mysqldump --version"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn database_server_probe_uses_ssh_peer_without_a_database_password()
-> Result<(), Box<dyn std::error::Error>> {
    let connection = DatabaseConnection {
        database_id: DatabaseId::parse("postgres-main")?,
        engine: DatabaseEngine::PostgreSql,
        host: "localhost".to_owned(),
        port: 5432,
        database_name: "app".to_owned(),
        authentication: DatabaseAuthentication::SshPeer,
    };
    let arguments = SystemOpenSsh::default().database_server_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        &connection,
    )?;
    let rendered = arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("psql --no-password"));
    assert!(rendered.contains("--host 'localhost' --port '5432' --dbname 'app'"));
    assert!(rendered.contains("SHOW server_version"));
    assert!(!rendered.contains("password="));
    assert!(!rendered.contains("PGPASSWORD"));
    Ok(())
}

#[test]
fn pin_and_capture_plan_fail_closed_on_untrusted_input() {
    assert!(PinnedHost::parse("vds.example", 22, "ssh-ed25519", "not base64!").is_err());
    assert!(PinnedHost::parse("vds.example", 22, "ssh-rsa", "c3NoLXJzYQ==").is_err());
    assert!(SshUser::parse("backup; whoami").is_err());
    assert!(RemoteCapturePlan::from_roots(["relative/path".to_owned()]).is_err());
    assert!(RemoteCapturePlan::from_roots(["/srv/../etc".to_owned()]).is_err());
    assert!(RemoteCapturePlan::from_roots(["/srv\nwhoami".to_owned()]).is_err());
}

#[test]
fn pin_serializes_the_exact_known_hosts_identity() -> Result<(), Box<dyn std::error::Error>> {
    let host = pinned_host()?;
    assert_eq!(
        host.known_hosts_line(),
        format!("vds.example ssh-ed25519 {}\n", ed25519_blob())
    );
    Ok(())
}

#[test]
fn failed_launch_removes_the_partial_capture() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("filesystem.tar.zst");
    let result = SystemOpenSsh::with_binary(directory.path().join("missing-ssh")).capture_to(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        &RemoteCapturePlan::from_roots(["/srv/app".to_owned()])?,
        &destination,
    );
    assert!(result.is_err());
    assert!(!destination.exists());
    Ok(())
}

fn pinned_host() -> Result<PinnedHost, Box<dyn std::error::Error>> {
    Ok(PinnedHost::parse(
        "vds.example",
        22,
        "ssh-ed25519",
        ed25519_blob(),
    )?)
}

fn ed25519_blob() -> String {
    let mut blob = Vec::new();
    blob.extend_from_slice(&11_u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.extend_from_slice(&[1]);
    STANDARD.encode(blob)
}
