use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::path::Path;

#[test]
fn push_filesystem_command_uses_the_atomic_rename_template()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().push_filesystem_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("target='/srv/app'"));
    assert!(rendered.contains("[ ! -e \"$target\" ] || exit 1"));
    assert!(rendered.contains("mkdir -- \"$tmp\" || exit 1"));
    assert!(rendered.contains(
        "tar --extract --file=- --zstd --numeric-owner --one-file-system -C \"$tmp\" --"
    ));
    assert!(rendered.contains("mv -n -- \"$tmp\" \"$target\""));
    assert!(rendered.contains("[ ! -e \"$tmp\" ] || status=1"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn push_database_command_guards_the_database_file_not_the_target_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().push_database_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("target='/srv/app/database.sqlite'"));
    assert!(rendered.contains("[ ! -e \"$target\" ] || exit 1"));
    assert!(rendered.contains("zstd -q -d -c > \"$tmp\""));
    assert!(rendered.contains("mv -n -- \"$tmp\" \"$target\""));
    // The database push must guard the file, never the directory itself --
    // a preceding filesystem push may have already legitimately created it.
    assert!(!rendered.contains("target='/srv/app'"));
    Ok(())
}

#[test]
fn push_commands_safely_quote_a_target_path_containing_a_single_quote()
-> Result<(), Box<dyn std::error::Error>> {
    let host = pinned_host()?;
    let user = SshUser::parse("backup")?;
    let identity = Path::new("C:/keys/backup.key");
    let known_hosts = Path::new("C:/known_hosts");
    let target = "/srv/app's data";

    let filesystem = SystemOpenSsh::default().push_filesystem_arguments(
        &host,
        &user,
        identity,
        known_hosts,
        target,
    );
    assert!(render(&filesystem).contains(r#"target='/srv/app'"'"'s data'"#));

    let database = SystemOpenSsh::default().push_database_arguments(
        &host,
        &user,
        identity,
        known_hosts,
        target,
    );
    assert!(render(&database).contains(r#"target='/srv/app'"'"'s data/database.sqlite'"#));
    Ok(())
}

#[test]
fn target_absence_probe_is_pinned_and_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().target_absence_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.ends_with("[ ! -e '/srv/app' ]"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn zstd_probe_is_pinned_and_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().zstd_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("command -v zstd >/dev/null 2>&1"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn failed_launch_returns_an_error_for_a_push() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let source = tempfile::NamedTempFile::new()?.reopen()?;
    let result = SystemOpenSsh::with_binary(directory.path().join("missing-ssh"))
        .push_filesystem_to(
            &pinned_host()?,
            &SshUser::parse("backup")?,
            Path::new("C:/keys/backup.key"),
            "/srv/app",
            source,
            0,
        );
    assert!(result.is_err());
    Ok(())
}

fn render(arguments: &[std::ffi::OsString]) -> String {
    arguments
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
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
