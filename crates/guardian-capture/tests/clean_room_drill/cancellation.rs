use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_core::{
    CancellationHandle, CredentialId, JobRegistry, ProfileId, RemoteTargetPath, RepositoryId,
    RunId, SecretStore, SecretValue,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::{error::Error, thread, time::Duration};

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn deploy_cancellation_drill() -> Result<(), Box<dyn Error>> {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let target = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    let stream_marker = target.install_throttled_deploy_key(&public_key, workdir.path())?;
    source.add_incompressible_fixture_file()?;
    let source_host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let target_host_key = target.host_key_base64(HOST_KEY_DEADLINE)?;
    let ready_ssh = SystemOpenSsh::default();
    let user = SshUser::parse("backup")?;
    let source_host = PinnedHost::parse(
        "127.0.0.1",
        source.port(),
        "ssh-ed25519",
        source_host_key.clone(),
    )?;
    let target_host = PinnedHost::parse(
        "127.0.0.1",
        target.port(),
        "ssh-ed25519",
        target_host_key.clone(),
    )?;
    support::wait_until_ssh_ready(
        &ready_ssh,
        &source_host,
        &user,
        &private_key,
        READY_DEADLINE,
    )?;
    support::wait_until_ssh_ready(
        &ready_ssh,
        &target_host,
        &user,
        &private_key,
        READY_DEADLINE,
    )?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let source_credential = CredentialId::parse("drill-cancel-source-credential")?;
    let target_credential = CredentialId::parse("drill-cancel-target-credential")?;
    let private_key_bytes = std::fs::read(&private_key)?;
    vault.store(
        &source_credential,
        &SecretValue::new(private_key_bytes.clone()),
    )?;
    vault.store(&target_credential, &SecretValue::new(private_key_bytes))?;
    let source_profile = support::drill_profile(
        ProfileId::parse("drill-cancel-source")?,
        source_credential,
        source.port(),
        &source_host_key,
    )?;
    let target_profile_id = ProfileId::parse("drill-cancel-target")?;
    let target_profile = support::drill_profile(
        target_profile_id.clone(),
        target_credential,
        target.port(),
        &target_host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-cancel-repository")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let signer = support::TestSigner::new();
    let capture = support::capture_drill_backup(
        &repository,
        &ready_ssh,
        &source_profile,
        &vault,
        &signer,
        "drill-cancel-backup",
        "drill-cancel-capture",
    )?;

    let handle = CancellationHandle::new();
    let deploy_ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let deployment = DeploymentComposition {
        repository: &repository,
        ssh: &deploy_ssh,
        target_profile: &target_profile,
        credentials: &vault,
        verifier: &signer,
    };
    let target_path = RemoteTargetPath::parse("/srv/drill-cancelled")?;
    let plan = deployment.plan(&capture.sealed.backup_id, target_path.clone())?;
    let run_id = RunId::parse("drill-deploy-cancel")?;
    let jobs = JobRegistry::default();
    let registration = jobs.register(run_id.clone(), handle);
    let confirmation = plan.confirmation.clone();

    thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        let deploy = scope.spawn(|| {
            deployment.execute(
                &run_id,
                &target_profile_id,
                &capture.sealed.backup_id,
                target_path,
                &confirmation,
            )
        });
        target.wait_for_remote_file(stream_marker, Duration::from_secs(15))?;
        if !jobs.cancel(&run_id) {
            return Err("operator cancellation did not find the running deploy".into());
        }
        let result = deploy
            .join()
            .map_err(|_| "cancelled deployment thread panicked")?;
        if result.is_ok() {
            return Err("deployment succeeded after operator cancellation".into());
        }
        Ok(())
    })?;
    drop(registration);

    target.wait_for_remote_absence(
        &[
            "/srv/drill-cancelled",
            "/srv/.guardian-deploy-staging.drill-deploy-cancel",
        ],
        Duration::from_secs(15),
    )?;
    let audit = repository.root().join("audit");
    assert!(
        audit
            .join("deploy-drill-deploy-cancel-attempted.json")
            .is_file()
    );
    assert!(
        audit
            .join("deploy-drill-deploy-cancel-cancelled.json")
            .is_file()
    );
    assert!(
        !audit
            .join("deploy-drill-deploy-cancel-completed.json")
            .exists()
    );
    assert!(
        !audit
            .join("deploy-drill-deploy-cancel-failed.json")
            .exists()
    );
    assert!(!jobs.cancel(&run_id));
    Ok(())
}
