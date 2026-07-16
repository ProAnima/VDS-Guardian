mod output;
mod parser;

use crate::secret_store::resolve_store;
use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::{RepositoryId, SecretStore};
use guardian_local_repository::{
    LocalRepository, RecoveryBundleError, RepositoryError, RepositoryVerificationKey,
    export_recovery_bundle, import_recovery_bundle,
};
use guardian_signing::{PortableVerificationKey, SigningIdentityManager};
use output::{RecoveryFailure, RecoveryOutput, write_error, write_success};
use parser::{RecoveryAction, RecoveryCommand, parse};
use std::{ffi::OsString, fs, path::Path, process::ExitCode};

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
    let passphrase = read_passphrase(
        command
            .passphrase_file
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )?;
    let output_path = command
        .output
        .as_deref()
        .ok_or_else(RecoveryFailure::usage)?;
    export_recovery_bundle(
        repository,
        secrets,
        repository_id,
        &passphrase,
        output_path,
        confirmation,
    )
    .map_err(map_bundle_error)?;
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
    let passphrase = read_passphrase(
        command
            .passphrase_file
            .as_deref()
            .ok_or_else(RecoveryFailure::usage)?,
    )?;
    let input = command
        .input
        .as_deref()
        .ok_or_else(RecoveryFailure::usage)?;
    let credential_id = import_recovery_bundle(
        repository,
        secrets,
        repository_id,
        &passphrase,
        input,
        confirmation,
    )
    .map_err(map_bundle_error)?;
    Ok(RecoveryOutput::Import { credential_id })
}

fn to_repository_key(key: &PortableVerificationKey) -> RepositoryVerificationKey {
    RepositoryVerificationKey {
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

fn map_bundle_error(error: RecoveryBundleError) -> RecoveryFailure {
    match error {
        RecoveryBundleError::ConfirmationMismatch => RecoveryFailure::confirmation_mismatch(),
        RecoveryBundleError::NotConfigured => RecoveryFailure::not_configured(),
        RecoveryBundleError::Repository(error) => map_install_error(error),
        RecoveryBundleError::InvalidBundle | RecoveryBundleError::Crypto => {
            RecoveryFailure::bundle_operation()
        }
        RecoveryBundleError::Io => RecoveryFailure::bundle_io(),
    }
}

fn read_passphrase(path: &Path) -> Result<Vec<u8>, RecoveryFailure> {
    const MAX_PASSPHRASE_FILE_BYTES: u64 = 4 * 1024;
    let metadata = fs::symlink_metadata(path).map_err(|_| RecoveryFailure::passphrase_input())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PASSPHRASE_FILE_BYTES
    {
        return Err(RecoveryFailure::passphrase_input());
    }
    let bytes = fs::read(path).map_err(|_| RecoveryFailure::passphrase_input())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| RecoveryFailure::passphrase_input())?;
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(RecoveryFailure::passphrase_input());
    }
    Ok(trimmed.as_bytes().to_vec())
}

#[cfg(test)]
mod tests;
