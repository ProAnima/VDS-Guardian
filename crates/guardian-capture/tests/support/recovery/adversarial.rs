use super::cli::{args, path, run_guardian_cli_failure};
use super::{
    RecoveryFixture, recovery_import_from, register, restore_args, restore_plan, vault_init,
};
use guardian_configuration::RepositoryStore;
use guardian_core::BackupId;
use guardian_local_repository::LocalRepository;
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

pub fn prove_hostile_restore_failures(
    root: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
    second_payload_failure_backup_id: &BackupId,
) -> Result<(), Box<dyn Error>> {
    wrong_passphrase_leaves_no_registration(root, fixture)?;
    missing_recovery_key_leaves_no_destination(root, fixture, backup_id)?;
    corrupted_payload_leaves_no_destination(root, fixture, backup_id)?;
    second_payload_failure_leaves_no_destination(root, fixture, second_payload_failure_backup_id)?;
    Ok(())
}

fn second_payload_failure_leaves_no_destination(
    root: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
) -> Result<(), Box<dyn Error>> {
    let repositories = root.join("second-payload-repositories");
    let vault = root.join("second-payload-vault");
    let signing = root.join("second-payload-signing");
    let destination = root.join("second-payload-destination");
    fs::create_dir(&repositories)?;
    vault_init(&vault)?;
    recovery_import_from(&repositories, &vault, fixture, &fixture.repository_path)?;
    expect_restore_failure(
        &repositories,
        &signing,
        &vault,
        fixture,
        backup_id,
        &destination,
    )?;
    assert_destination_absent(&destination, "failed second payload")?;
    assert_no_restore_staging(root)
}

fn wrong_passphrase_leaves_no_registration(
    root: &Path,
    fixture: &RecoveryFixture,
) -> Result<(), Box<dyn Error>> {
    let repositories = root.join("wrong-passphrase-repositories");
    let vault = root.join("wrong-passphrase-vault");
    let passphrase = root.join("wrong-passphrase.txt");
    fs::create_dir(&repositories)?;
    fs::write(&passphrase, b"definitely not the recovery passphrase")?;
    vault_init(&vault)?;
    let confirmation = format!(
        "IMPORT RECOVERY BUNDLE FOR {}",
        fixture.repository_id.as_str()
    );
    run_guardian_cli_failure(args(&[
        "recovery",
        "import",
        "--repositories-dir",
        &path(&repositories),
        "--repository-id",
        fixture.repository_id.as_str(),
        "--repository-path",
        &path(&fixture.repository_path),
        "--vault-dir",
        &path(&vault),
        "--passphrase-file",
        &path(&passphrase),
        "--input",
        &path(&fixture.bundle_path),
        "--confirmation",
        &confirmation,
        "--json",
    ]))?;
    if RepositoryStore::at(&repositories)
        .get(&fixture.repository_id)?
        .is_some()
    {
        return Err("failed recovery import left a repository registration".into());
    }
    Ok(())
}

fn missing_recovery_key_leaves_no_destination(
    root: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
) -> Result<(), Box<dyn Error>> {
    let repositories = root.join("missing-key-repositories");
    let vault = root.join("missing-key-vault");
    let signing = root.join("missing-key-signing");
    let destination = root.join("missing-key-destination");
    fs::create_dir(&repositories)?;
    vault_init(&vault)?;
    let repository =
        LocalRepository::open(&fixture.repository_path, fixture.repository_id.clone())?;
    register(&repositories, &repository, fixture.repository_id.clone())?;
    expect_restore_failure(
        &repositories,
        &signing,
        &vault,
        fixture,
        backup_id,
        &destination,
    )?;
    assert_destination_absent(&destination, "missing recovery key")
}

fn corrupted_payload_leaves_no_destination(
    root: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
) -> Result<(), Box<dyn Error>> {
    let repository_path = root.join("corrupted-repository");
    copy_directory(&fixture.repository_path, &repository_path)?;
    flip_last_byte(
        &repository_path
            .join("backups")
            .join(backup_id.as_str())
            .join("payload")
            .join("filesystem-000.tar.zst.enc"),
    )?;
    let repositories = root.join("corrupted-repositories");
    let vault = root.join("corrupted-vault");
    let signing = root.join("corrupted-signing");
    let destination = root.join("corrupted-destination");
    fs::create_dir(&repositories)?;
    vault_init(&vault)?;
    recovery_import_from(&repositories, &vault, fixture, &repository_path)?;
    run_guardian_cli_failure(args(&restore_args(
        "plan",
        &repositories,
        &signing,
        &vault,
        &fixture.repository_id,
        backup_id,
        &destination,
    )))?;
    assert_destination_absent(&destination, "corrupted encrypted payload")
}

fn expect_restore_failure(
    repositories: &Path,
    signing: &Path,
    vault: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    let plan = restore_plan(
        repositories,
        signing,
        vault,
        &fixture.repository_id,
        backup_id,
        destination,
    )?;
    let confirmation = plan
        .pointer("/data/confirmation")
        .and_then(serde_json::Value::as_str)
        .ok_or("restore plan omitted its confirmation phrase")?;
    let mut values = restore_args(
        "execute",
        repositories,
        signing,
        vault,
        &fixture.repository_id,
        backup_id,
        destination,
    );
    values.extend(["--confirmation".to_owned(), confirmation.to_owned()]);
    run_guardian_cli_failure(args(&values))
}

fn assert_destination_absent(destination: &Path, scenario: &str) -> Result<(), Box<dyn Error>> {
    if destination.exists() {
        return Err(format!("{scenario} published a partial restore destination").into());
    }
    Ok(())
}

fn assert_no_restore_staging(parent: &Path) -> Result<(), Box<dyn Error>> {
    let staging_exists = fs::read_dir(parent)?.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with(".guardian-restore-tmp-")
    });
    if staging_exists {
        return Err("failed second payload left restore staging behind".into());
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn flip_last_byte(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)?;
    file.seek(SeekFrom::End(-1))?;
    file.write_all(&[byte[0] ^ 0xff])?;
    file.sync_all()?;
    Ok(())
}
