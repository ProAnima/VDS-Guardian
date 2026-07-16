use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_core::{
    CredentialId, ProfileId, RemoteTargetPath, RepositoryId, RunId, SecretStore, SecretValue,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::{error::Error, time::Duration};

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn failed_database_push_removes_remote_deploy_staging() -> Result<(), Box<dyn Error>> {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let target = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    target.install_public_key(&public_key)?;
    let source_host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let target_host_key = target.host_key_base64(HOST_KEY_DEADLINE)?;
    let ssh = SystemOpenSsh::default();
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
    support::wait_until_ssh_ready(&ssh, &source_host, &user, &private_key, READY_DEADLINE)?;
    support::wait_until_ssh_ready(&ssh, &target_host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let source_credential = CredentialId::parse("drill-deploy-failure-source-credential")?;
    let target_credential = CredentialId::parse("drill-deploy-failure-target-credential")?;
    let private_key_bytes = std::fs::read(&private_key)?;
    vault.store(
        &source_credential,
        &SecretValue::new(private_key_bytes.clone()),
    )?;
    vault.store(&target_credential, &SecretValue::new(private_key_bytes))?;
    let source_profile = support::drill_profile(
        ProfileId::parse("drill-deploy-failure-source")?,
        source_credential,
        source.port(),
        &source_host_key,
    )?;
    let target_profile_id = ProfileId::parse("drill-deploy-failure-target")?;
    let target_profile = support::drill_profile(
        target_profile_id.clone(),
        target_credential,
        target.port(),
        &target_host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-deploy-failure-repository")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let signer = support::TestSigner::new();
    let captured = support::capture_drill_backup(
        &repository,
        &ssh,
        &source_profile,
        &vault,
        &signer,
        "drill-deploy-failure-capture",
        "drill-deploy-failure-capture-run",
    )?;
    let backup = support::create_second_payload_failure_backup(
        &repository,
        &captured.sealed.backup_id,
        &vault,
        &signer,
        &source_profile,
    )?;
    let deployment = DeploymentComposition {
        repository: &repository,
        ssh: &ssh,
        target_profile: &target_profile,
        credentials: &vault,
        verifier: &signer,
    };
    let target_path = RemoteTargetPath::parse("/srv/drill-deploy-failure")?;
    let plan = deployment.plan(&backup.backup_id, target_path.clone())?;
    let run_id = RunId::parse("drill-deploy-second-failure")?;
    if deployment
        .execute(
            &run_id,
            &target_profile_id,
            &backup.backup_id,
            target_path,
            &plan.confirmation,
        )
        .is_ok()
    {
        return Err("deploy succeeded with an invalid database payload".into());
    }

    target.wait_for_remote_absence(
        &[
            "/srv/drill-deploy-failure",
            "/srv/.guardian-deploy-staging.drill-deploy-second-failure",
        ],
        Duration::from_secs(15),
    )?;
    let audit = repository.root().join("audit");
    assert!(
        audit
            .join("deploy-drill-deploy-second-failure-attempted.json")
            .is_file()
    );
    assert!(
        audit
            .join("deploy-drill-deploy-second-failure-failed.json")
            .is_file()
    );
    assert!(
        !audit
            .join("deploy-drill-deploy-second-failure-completed.json")
            .exists()
    );
    assert!(
        !audit
            .join("deploy-drill-deploy-second-failure-cancelled.json")
            .exists()
    );
    Ok(())
}
