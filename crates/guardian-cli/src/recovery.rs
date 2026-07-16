mod bundle;
mod output;
mod parser;

use crate::secret_store::resolve_store;
use bundle::{RecoveryBundleFile, read_bundle_file, read_passphrase, write_bundle_file};
use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::{RepositoryId, SecretStore};
use guardian_encryption::recovery_bundle::{self, KdfParams};
use guardian_local_repository::{LocalRepository, RepositoryError, RepositoryVerificationKey};
use guardian_signing::{Ed25519Verifier, PortableVerificationKey, SigningIdentityManager};
use output::{RecoveryFailure, RecoveryOutput, write_error, write_success};
use parser::{RecoveryAction, RecoveryCommand, parse};
use std::{ffi::OsString, process::ExitCode};

pub(super) fn run(arguments: &[OsString]) -> ExitCode {
    match parse(arguments).and_then(|command| {
        let store =
            resolve_store(command.vault_dir.as_deref()).map_err(|_| RecoveryFailure::store())?;
        execute(command, &store)
    }) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn execute(
    command: RecoveryCommand,
    secrets: &dyn SecretStore,
) -> Result<RecoveryOutput, RecoveryFailure> {
    let repository_id =
        RepositoryId::parse(&command.repository_id).map_err(|_| RecoveryFailure::input())?;
    let (repository, pending_registration) = resolve_repository(&command, &repository_id)?;
    match command.action {
        RecoveryAction::Init => execute_init(&repository, secrets, &command),
        RecoveryAction::Status => match repository.export_recovery_key(secrets) {
            Ok(Some(_)) => repository
                .trusted_verification_key()
                .map(|key| RecoveryOutput::Status {
                    configured: key.is_some(),
                })
                .map_err(|_| RecoveryFailure::storage()),
            Ok(None) | Err(RepositoryError::Credential) => {
                Ok(RecoveryOutput::Status { configured: false })
            }
            Err(_) => Err(RecoveryFailure::storage()),
        },
        RecoveryAction::Export => execute_export(&repository, secrets, &command, &repository_id),
        RecoveryAction::Import => {
            let output = execute_import(&repository, secrets, &command, &repository_id)?;
            if let Some(registration) = pending_registration {
                RepositoryStore::at(&command.repositories_dir)
                    .upsert(registration)
                    .map_err(|_| RecoveryFailure::storage())?;
            }
            Ok(output)
        }
    }
}

fn resolve_repository(
    command: &RecoveryCommand,
    repository_id: &RepositoryId,
) -> Result<(LocalRepository, Option<RepositoryRegistration>), RecoveryFailure> {
    let store = RepositoryStore::at(&command.repositories_dir);
    if let Some(registration) = store
        .get(repository_id)
        .map_err(|_| RecoveryFailure::storage())?
    {
        return LocalRepository::open(&registration.path, repository_id.clone())
            .map(|repository| (repository, None))
            .map_err(|_| RecoveryFailure::storage());
    }
    if !matches!(command.action, RecoveryAction::Import) {
        return Err(RecoveryFailure::input());
    }
    let path = command
        .repository_path
        .as_deref()
        .ok_or_else(RecoveryFailure::input)?;
    let repository = LocalRepository::open(path, repository_id.clone())
        .map_err(|_| RecoveryFailure::storage())?;
    let registration = RepositoryRegistration::new(
        repository_id.clone(),
        format!("Recovered {}", repository_id.as_str()),
        repository.root().to_owned(),
    )
    .map_err(|_| RecoveryFailure::input())?;
    Ok((repository, Some(registration)))
}

fn execute_init(
    repository: &LocalRepository,
    secrets: &dyn SecretStore,
    command: &RecoveryCommand,
) -> Result<RecoveryOutput, RecoveryFailure> {
    let manager = SigningIdentityManager::open(
        command
            .signing_config_dir
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )
    .map_err(|_| RecoveryFailure::signing())?;
    let identity = manager
        .load_ready(secrets)
        .map_err(|_| RecoveryFailure::signing())?;
    repository
        .pin_verification_key(to_repository_key(&identity.verification_key()))
        .map_err(map_install_error)?;
    match repository.configure_recovery_key(secrets) {
        Ok(credential_id) => Ok(RecoveryOutput::Init { credential_id }),
        Err(RepositoryError::RecoveryKeyAlreadyConfigured) => repository
            .recovery_credential_id()
            .map_err(map_install_error)?
            .map(|credential_id| RecoveryOutput::Init { credential_id })
            .ok_or_else(RecoveryFailure::not_configured),
        Err(error) => Err(map_install_error(error)),
    }
}

fn execute_export(
    repository: &LocalRepository,
    secrets: &dyn SecretStore,
    command: &RecoveryCommand,
    repository_id: &RepositoryId,
) -> Result<RecoveryOutput, RecoveryFailure> {
    let confirmation = command
        .confirmation
        .as_deref()
        .ok_or_else(RecoveryFailure::usage)?;
    if confirmation != export_confirmation_phrase(repository_id) {
        return Err(RecoveryFailure::confirmation_mismatch());
    }
    let key = repository
        .export_recovery_key(secrets)
        .map_err(|_| RecoveryFailure::storage())?
        .ok_or_else(RecoveryFailure::not_configured)?;
    let verification_key = repository
        .trusted_verification_key()
        .map_err(|_| RecoveryFailure::storage())?
        .ok_or_else(RecoveryFailure::signing)?;
    let passphrase = read_passphrase(
        command
            .passphrase_file
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )?;
    let params = KdfParams::recommended();
    let binding = bundle_binding(repository_id, &verification_key);
    let wrapped = recovery_bundle::wrap_recovery_key(&passphrase, &key, &binding, params)
        .map_err(|_| RecoveryFailure::bundle_operation())?;
    let output_path = command
        .output
        .as_deref()
        .ok_or_else(RecoveryFailure::usage)?;
    write_bundle_file(
        output_path,
        &RecoveryBundleFile::from_wrapped(&wrapped, params, verification_key),
    )?;
    Ok(RecoveryOutput::Export {
        output: output_path.display().to_string(),
    })
}

fn execute_import(
    repository: &LocalRepository,
    secrets: &dyn SecretStore,
    command: &RecoveryCommand,
    repository_id: &RepositoryId,
) -> Result<RecoveryOutput, RecoveryFailure> {
    let confirmation = command
        .confirmation
        .as_deref()
        .ok_or_else(RecoveryFailure::usage)?;
    if confirmation != import_confirmation_phrase(repository_id) {
        return Err(RecoveryFailure::confirmation_mismatch());
    }
    let bundle = read_bundle_file(
        command
            .input
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )?;
    let verification_key = bundle.verification_key();
    Ed25519Verifier::from_portable(&to_portable_key(&verification_key))
        .map_err(|_| RecoveryFailure::bundle_operation())?;
    if repository
        .trusted_verification_key()
        .map_err(|_| RecoveryFailure::storage())?
        .is_some_and(|existing| existing != verification_key)
    {
        return Err(RecoveryFailure::bundle_operation());
    }
    let passphrase = read_passphrase(
        command
            .passphrase_file
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )?;
    let params = bundle.kdf_params();
    let wrapped = bundle.to_wrapped()?;
    let binding = bundle_binding(repository_id, &verification_key);
    let key = recovery_bundle::unwrap_recovery_key(&passphrase, &wrapped, &binding, params)
        .map_err(|_| RecoveryFailure::bundle_operation())?;
    let output = repository
        .import_recovery_key(secrets, key)
        .map(|credential_id| RecoveryOutput::Import { credential_id })
        .map_err(map_install_error)?;
    repository
        .pin_verification_key(verification_key)
        .map_err(map_install_error)?;
    Ok(output)
}

fn bundle_binding(repository_id: &RepositoryId, key: &RepositoryVerificationKey) -> String {
    format!(
        "{}|{}|{}|{}",
        repository_id.as_str(),
        key.algorithm,
        key.key_id,
        key.public_key_base64
    )
}

fn to_repository_key(key: &PortableVerificationKey) -> RepositoryVerificationKey {
    RepositoryVerificationKey {
        algorithm: key.algorithm.clone(),
        key_id: key.key_id.clone(),
        public_key_base64: key.public_key_base64.clone(),
    }
}

fn to_portable_key(key: &RepositoryVerificationKey) -> PortableVerificationKey {
    PortableVerificationKey {
        algorithm: key.algorithm.clone(),
        key_id: key.key_id.clone(),
        public_key_base64: key.public_key_base64.clone(),
    }
}

fn map_install_error(error: RepositoryError) -> RecoveryFailure {
    match error {
        RepositoryError::RecoveryKeyAlreadyConfigured => RecoveryFailure::already_configured(),
        RepositoryError::RecoveryKeyMismatch | RepositoryError::TrustedSigningKeyMismatch => {
            RecoveryFailure::bundle_operation()
        }
        _ => RecoveryFailure::storage(),
    }
}

fn export_confirmation_phrase(repository_id: &RepositoryId) -> String {
    format!("EXPORT RECOVERY BUNDLE FOR {}", repository_id.as_str())
}

fn import_confirmation_phrase(repository_id: &RepositoryId) -> String {
    format!("IMPORT RECOVERY BUNDLE FOR {}", repository_id.as_str())
}

#[cfg(test)]
mod tests;
