use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_core::RunId;
use guardian_ssh::{PinnedHost, ReplacementTarget, SshUser, StagingTarget, SystemOpenSsh};
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
fn push_filesystem_into_staging_command_never_renames_into_place()
-> Result<(), Box<dyn std::error::Error>> {
    let run_id = RunId::parse("run-staging-fs")?;
    let arguments = SystemOpenSsh::default().push_filesystem_into_staging_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        StagingTarget {
            target_path: "/srv/app",
            run_id: &run_id,
        },
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("target='/srv/app'"));
    assert!(rendered.contains("staging=\"$parent/.guardian-deploy-staging.run-staging-fs\""));
    assert!(rendered.contains("[ ! -e \"$target\" ] || exit 1"));
    assert!(rendered.contains("[ ! -e \"$staging\" ] || exit 1"));
    assert!(rendered.contains("mkdir -- \"$staging\" || exit 1"));
    assert!(rendered.contains("chmod 755 -- \"$staging\" || exit 1"));
    assert!(rendered.contains(
        "tar --extract --file=- --zstd --no-same-owner --no-same-permissions --one-file-system -C \"$staging\" --"
    ));
    // The whole point of staging: this command never renames anything into
    // place, so a database push that never runs (or fails) can never leave
    // `target` half-populated.
    assert!(!rendered.contains("mv -n"));
    Ok(())
}

#[test]
fn push_database_into_staging_command_requires_an_existing_staging_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let run_id = RunId::parse("run-staging-db")?;
    let arguments = SystemOpenSsh::default().push_database_into_staging_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        StagingTarget {
            target_path: "/srv/app",
            run_id: &run_id,
        },
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("target='/srv/app'"));
    assert!(rendered.contains("staging=\"$parent/.guardian-deploy-staging.run-staging-db\""));
    assert!(rendered.contains("[ -d \"$staging\" ] || exit 1"));
    assert!(rendered.contains("zstd -q -d -c > \"$staging/database.sqlite\""));
    // On its own failure this cleans up the *whole* staging tree, not just
    // the database file it was writing -- a failed second stage abandons
    // the entire attempt, including the filesystem payload already staged.
    assert!(rendered.contains("rm -rf -- \"$staging\""));
    assert!(!rendered.contains("mv -n"));
    Ok(())
}

#[test]
fn finalize_deploy_command_publishes_the_staging_directory_with_one_rename()
-> Result<(), Box<dyn std::error::Error>> {
    let run_id = RunId::parse("run-staging-finalize")?;
    let arguments = SystemOpenSsh::default().finalize_deploy_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        StagingTarget {
            target_path: "/srv/app",
            run_id: &run_id,
        },
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("target='/srv/app'"));
    assert!(rendered.contains("staging=\"$parent/.guardian-deploy-staging.run-staging-finalize\""));
    assert!(rendered.contains("[ -e \"$staging\" ] || exit 1"));
    assert!(rendered.contains("[ ! -e \"$target\" ] || exit 1"));
    assert!(rendered.contains("mv -n -- \"$staging\" \"$target\""));
    assert!(rendered.contains("[ ! -e \"$staging\" ] || status=1"));
    Ok(())
}

#[test]
fn the_three_staging_commands_agree_on_the_same_staging_path_for_one_run_id()
-> Result<(), Box<dyn std::error::Error>> {
    // The three separate SSH invocations of one combined deploy attempt
    // never share process state -- they can only agree on the staging
    // directory's name because all three render it from the exact same
    // `run_id`. Prove they can't drift onto different naming schemes.
    let host = pinned_host()?;
    let user = SshUser::parse("backup")?;
    let identity = Path::new("C:/keys/backup.key");
    let known_hosts = Path::new("C:/known_hosts");
    let run_id = RunId::parse("run-staging-agree")?;
    let ssh = SystemOpenSsh::default();
    let staging = StagingTarget {
        target_path: "/srv/app",
        run_id: &run_id,
    };
    let stage_fs = render(&ssh.push_filesystem_into_staging_arguments(
        &host,
        &user,
        identity,
        known_hosts,
        staging,
    ));
    let stage_db = render(&ssh.push_database_into_staging_arguments(
        &host,
        &user,
        identity,
        known_hosts,
        staging,
    ));
    let finalize =
        render(&ssh.finalize_deploy_arguments(&host, &user, identity, known_hosts, staging));
    let staging_assignment = "staging=\"$parent/.guardian-deploy-staging.run-staging-agree\"";
    assert!(stage_fs.contains(staging_assignment));
    assert!(stage_db.contains(staging_assignment));
    assert!(finalize.contains(staging_assignment));
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

    let run_id = RunId::parse("run-quoting")?;
    let staging = SystemOpenSsh::default().push_filesystem_into_staging_arguments(
        &host,
        &user,
        identity,
        known_hosts,
        StagingTarget {
            target_path: target,
            run_id: &run_id,
        },
    );
    assert!(render(&staging).contains(r#"target='/srv/app'"'"'s data'"#));
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
fn replacement_stages_then_swaps_with_a_preserved_rollback()
-> Result<(), Box<dyn std::error::Error>> {
    let run_id = RunId::parse("replace-001")?;
    let containers = vec!["web".to_owned(), "worker".to_owned()];
    let target = ReplacementTarget {
        source_root: "/srv/app/data",
        run_id: &run_id,
        containers: &containers,
    };
    let ssh = SystemOpenSsh::default();
    let stage = render(&ssh.replacement_staging_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        target,
    ));
    assert!(stage.contains("root='/srv/app/data'"));
    assert!(stage.contains(".guardian-replace-staging.replace-001"));
    assert!(stage.contains("source=\"$staging/${root#/}\""));
    assert!(!stage.contains("rm -rf -- \"$root\""));

    let commit = render(&ssh.commit_replacement_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        target,
    ));
    assert!(commit.contains("docker stop -- 'web' 'worker'"));
    assert!(commit.contains("mv -- \"$root\" \"$rollback\""));
    assert!(commit.contains("mv -- \"$rollback\" \"$root\""));
    assert!(commit.contains("trap 'rollback_cutover' HUP INT TERM"));
    assert!(commit.contains("attempts=0"));
    assert!(commit.contains("exit 42"));
    assert!(commit.contains("exit 43"));
    assert!(!commit.contains("rm -rf -- \"$rollback\""));
    Ok(())
}

#[test]
fn replacement_preflight_is_pinned_and_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = SystemOpenSsh::default().replacement_ready_probe_arguments(
        &pinned_host()?,
        &SshUser::parse("backup")?,
        Path::new("C:/keys/backup.key"),
        Path::new("C:/known_hosts"),
        "/srv/app/data",
    );
    let rendered = render(&arguments);
    assert!(rendered.contains("StrictHostKeyChecking=yes"));
    assert!(rendered.contains("[ -d \"$root\" ]"));
    assert!(rendered.contains("[ -w \"$parent\" ]"));
    assert!(!rendered.contains("mkdir"));
    assert!(!rendered.contains("mv --"));
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
