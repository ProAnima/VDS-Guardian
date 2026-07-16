use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_archive::ArchiveLimits;
use guardian_capture::{DiskSpacePort, FilesystemCaptureComposition};
use guardian_core::{
    CredentialId, FilesystemBackupRequest, FilesystemCaptureRequest, PayloadPath, ProfileId,
    RepositoryId, RunId, SecretStore, SecretValue, StoragePortError, Timestamp,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::{error::Error, path::Path};

struct ExhaustedDisk;

impl DiskSpacePort for ExhaustedDisk {
    fn available_space(&self, _: &Path) -> Result<u64, StoragePortError> {
        Ok(0)
    }
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn exhausted_disk_rejects_capture_before_staging() -> Result<(), Box<dyn Error>> {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    let host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let ssh = SystemOpenSsh::default();
    let user = SshUser::parse("backup")?;
    let host = PinnedHost::parse("127.0.0.1", source.port(), "ssh-ed25519", host_key.clone())?;
    support::wait_until_ssh_ready(&ssh, &host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let credential = CredentialId::parse("drill-exhausted-disk-credential")?;
    vault.store(&credential, &SecretValue::new(std::fs::read(&private_key)?))?;
    let profile = support::drill_profile(
        ProfileId::parse("drill-exhausted-disk-source")?,
        credential,
        source.port(),
        &host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-exhausted-disk-repository")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let audit = support::NoopAudit;
    let disk_space = ExhaustedDisk;
    let capture = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &ssh,
        profile: &profile,
        credentials: &vault,
        audit: &audit,
        disk_space: &disk_space,
        archive_limits: ArchiveLimits::conservative(),
    };
    let run_id = RunId::parse("drill-exhausted-disk")?;
    let backup_id = "drill-exhausted-disk-backup";
    let request = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?,
        },
        manifest: support::drill_manifest(backup_id, run_id.clone(), &profile)?,
        sealed_at: Timestamp::parse("2026-07-16T12:00:01Z")?,
    };
    if capture
        .execute(request, None, &support::TestSigner::new())
        .is_ok()
    {
        return Err("capture succeeded with no free repository disk space".into());
    }

    let root = repository.root();
    assert!(!root.join("staging").join(run_id.as_str()).exists());
    assert!(!root.join("backups").join(backup_id).exists());
    let audit = root.join("audit");
    assert!(
        audit
            .join("capture-drill-exhausted-disk-started.json")
            .is_file()
    );
    assert!(
        audit
            .join("capture-drill-exhausted-disk-failed.json")
            .is_file()
    );
    assert!(
        !audit
            .join("capture-drill-exhausted-disk-sealed.json")
            .exists()
    );
    Ok(())
}
