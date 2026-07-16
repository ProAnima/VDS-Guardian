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
    assert!(rendered.contains("mktemp -d -- \"$parent/.guardian-deploy-tmp.XXXXXX\") || exit 1"));
    assert!(rendered.contains("chmod 755 -- \"$tmp\" || exit 1"));
    assert!(rendered.contains(
        "tar --extract --file=- --zstd --no-same-owner --no-same-permissions --one-file-system -C \"$tmp\" --"
    ));
    assert!(rendered.contains("mv -n -- \"$tmp\" \"$target\""));
    assert!(rendered.contains("[ ! -e \"$tmp\" ] || status=1"));
    assert!(!rendered.contains("accept-new"));
    Ok(())
}

#[test]
fn push_filesystem_command_restores_an_ordinary_mode_after_mktemp()
-> Result<(), Box<dyn std::error::Error>> {
    // `mktemp -d` always creates its directory `0700` regardless of the
    // remote umask -- that restriction is the whole point of `mktemp` -- and
    // `mv -n` renames it as-is into the final target, so without an explicit
    // `chmod` the *deployed directory itself* (not its extracted contents,
    // which `--no-same-permissions` already governs) would silently end up
    // owner-only and lock out whatever account the deployed tree is meant
    // to actually serve.
    let arguments = SystemOpenSsh::default().push_filesystem_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    let mktemp_position = rendered
        .find("mktemp -d --")
        .ok_or("push_filesystem_command must create its temp directory via mktemp -d")?;
    let chmod_position = rendered
        .find("chmod 755 -- \"$tmp\"")
        .ok_or("push_filesystem_command must restore an ordinary mode on its temp directory")?;
    let tar_position = rendered
        .find("tar --extract")
        .ok_or("push_filesystem_command must extract via tar")?;
    assert!(mktemp_position < chmod_position);
    assert!(chmod_position < tar_position);
    Ok(())
}

#[test]
fn push_filesystem_command_never_deletes_before_creating_its_own_temp_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().push_filesystem_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    // The old scheme built a fixed, guessable sibling name and unconditionally
    // `rm -rf`'d it before ever creating anything -- a real risk to any
    // legitimate, unrelated thing that happened to already exist there.
    // `mktemp -d` both names and creates a fresh, unique directory in one
    // step, so the only correct place for an `rm -rf` of that exact path is
    // in the failure-cleanup branch, strictly after `mktemp` has already run.
    let mktemp_position = rendered
        .find("mktemp -d --")
        .ok_or("push_filesystem_command must create its temp directory via mktemp -d")?;
    if let Some(rm_rf_position) = rendered.find("rm -rf -- \"$tmp\"") {
        assert!(rm_rf_position > mktemp_position);
    }
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
    assert!(rendered.contains("chmod 644 -- \"$tmp\" || exit 1"));
    assert!(rendered.contains("zstd -q -d -c > \"$tmp\""));
    assert!(rendered.contains("mv -n -- \"$tmp\" \"$target\""));
    // The database push must guard the file, never the directory itself --
    // a preceding filesystem push may have already legitimately created it.
    assert!(!rendered.contains("target='/srv/app'"));
    Ok(())
}

#[test]
fn push_database_command_restores_an_ordinary_mode_after_mktemp()
-> Result<(), Box<dyn std::error::Error>> {
    // Bare `mktemp` always creates its file `0600` regardless of the remote
    // umask, and `mv -n` renames it as-is -- without an explicit `chmod` the
    // deployed database file would silently end up owner-only instead of the
    // umask-based mode a plain shell redirect used to leave it with.
    let arguments = SystemOpenSsh::default().push_database_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app",
    );
    let rendered = render(&arguments);
    let mktemp_position = rendered
        .find("mktemp --")
        .ok_or("push_database_command must create its temp file via mktemp")?;
    let chmod_position = rendered
        .find("chmod 644 -- \"$tmp\"")
        .ok_or("push_database_command must restore an ordinary mode on its temp file")?;
    let zstd_position = rendered
        .find("zstd -q -d -c")
        .ok_or("push_database_command must decompress via zstd")?;
    assert!(mktemp_position < chmod_position);
    assert!(chmod_position < zstd_position);
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
