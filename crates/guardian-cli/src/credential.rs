use fs2::FileExt;
use guardian_core::{CredentialId, SecretStore, SecretValue};
use guardian_ssh::SecretIdentityFile;
use serde::Serialize;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::ExitCode,
};

pub(super) fn run(arguments: &[OsString], store: &dyn SecretStore) -> ExitCode {
    match parse(arguments).and_then(|command| execute(command, store)) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<ImportCommand, CredentialFailure> {
    if arguments.first().and_then(|value| value.to_str()) != Some("import-ssh-key") {
        return Err(CredentialFailure::usage());
    }
    let mut credential_id = None;
    let mut input = None;
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--credential-id") => {
                index += 1;
                credential_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--input") => {
                index += 1;
                input = arguments.get(index).map(PathBuf::from);
            }
            Some("--json") => json = true,
            _ => return Err(CredentialFailure::usage()),
        }
        index += 1;
    }
    let credential_id = credential_id
        .ok_or_else(CredentialFailure::usage)
        .and_then(|value| CredentialId::parse(value).map_err(|_| CredentialFailure::usage()))?;
    let input = input.ok_or_else(CredentialFailure::usage)?;
    if !json || !input.is_absolute() {
        return Err(CredentialFailure::usage());
    }
    Ok(ImportCommand {
        credential_id,
        input,
    })
}

fn execute(
    command: ImportCommand,
    store: &dyn SecretStore,
) -> Result<CredentialOutput, CredentialFailure> {
    execute_with_lock_directory(command, store, &credential_lock_directory()?)
}

fn execute_with_lock_directory(
    command: ImportCommand,
    store: &dyn SecretStore,
    lock_directory: &Path,
) -> Result<CredentialOutput, CredentialFailure> {
    let key = read_key(&command.input)?;
    SecretIdentityFile::validate(key.expose()).map_err(|_| CredentialFailure::invalid_key())?;
    let _lock = credential_lock(lock_directory, &command.credential_id)?;
    if store
        .load(&command.credential_id)
        .map_err(|_| CredentialFailure::store())?
        .is_some()
    {
        return Err(CredentialFailure::already_exists());
    }
    store
        .store(&command.credential_id, &key)
        .map_err(|_| CredentialFailure::store())?;
    let stored = store
        .load(&command.credential_id)
        .map_err(|_| CredentialFailure::store())?
        .ok_or_else(CredentialFailure::store)?;
    SecretIdentityFile::validate(stored.expose()).map_err(|_| CredentialFailure::store())?;
    Ok(CredentialOutput {
        credential_id: command.credential_id,
    })
}

fn credential_lock(directory: &Path, id: &CredentialId) -> Result<File, CredentialFailure> {
    fs::create_dir_all(directory).map_err(|_| CredentialFailure::store())?;
    let metadata = fs::symlink_metadata(directory).map_err(|_| CredentialFailure::store())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CredentialFailure::store());
    }
    let path = directory.join(format!("{}.lock", id.as_str()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| CredentialFailure::store())?;
    let metadata = file.metadata().map_err(|_| CredentialFailure::store())?;
    if !metadata.is_file()
        || fs::symlink_metadata(&path)
            .map_err(|_| CredentialFailure::store())?
            .file_type()
            .is_symlink()
    {
        return Err(CredentialFailure::store());
    }
    file.lock_exclusive()
        .map_err(|_| CredentialFailure::store())?;
    Ok(file)
}

fn credential_lock_directory() -> Result<PathBuf, CredentialFailure> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
    } else {
        std::env::var_os("XDG_STATE_HOME").or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state").into_os_string())
        })
    }
    .map(PathBuf::from)
    .ok_or_else(CredentialFailure::store)?;
    Ok(base
        .join("ProAnima")
        .join("VDSGuardian")
        .join("credential-locks"))
}

fn read_key(path: &Path) -> Result<SecretValue, CredentialFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialFailure::input())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
        return Err(CredentialFailure::input());
    }
    fs::read(path)
        .map(SecretValue::new)
        .map_err(|_| CredentialFailure::input())
}

fn write_success(output: &CredentialOutput) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&CredentialFailure::serialization()),
    }
}

fn write_error(error: &CredentialFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

struct ImportCommand {
    credential_id: CredentialId,
    input: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialOutput {
    credential_id: CredentialId,
}

#[derive(Serialize)]
struct Success<'a, T> {
    ok: bool,
    data: &'a T,
}

#[derive(Serialize)]
struct Failure<'a, T> {
    ok: bool,
    error: &'a T,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl CredentialFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli credential import-ssh-key --credential-id <id> --input <absolute-key-path> --json",
        }
    }

    fn input() -> Self {
        Self {
            code: "invalid_key_input",
            message: "The SSH key input is not a safe regular file.",
            usage: "Provide an absolute non-symlink key file no larger than 64 KiB.",
        }
    }

    fn invalid_key() -> Self {
        Self {
            code: "invalid_ssh_key",
            message: "The key is not a supported unencrypted private key.",
            usage: "Create a dedicated unencrypted OpenSSH or PEM private key and protect it through the OS credential store.",
        }
    }

    fn already_exists() -> Self {
        Self {
            code: "credential_already_exists",
            message: "The credential already exists and was not replaced.",
            usage: "Use a new credential ID; explicit credential rotation is not implemented yet.",
        }
    }

    fn store() -> Self {
        Self {
            code: "credential_store_unavailable",
            message: "The secure credential store could not complete the request.",
            usage: "Unlock or configure the operating-system credential store and retry.",
        }
    }

    fn serialization() -> Self {
        Self {
            code: "serialization_failed",
            message: "The JSON response could not be serialized.",
            usage: "Retry and export redacted diagnostics if the problem persists.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialFailure, ImportCommand, execute, execute_with_lock_directory, parse};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{
        collections::HashMap,
        ffi::OsString,
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    #[test]
    fn import_requires_explicit_json_and_absolute_key_path() {
        for arguments in [
            vec!["import-ssh-key"],
            vec![
                "import-ssh-key",
                "--credential-id",
                "id",
                "--input",
                "key",
                "--json",
            ],
        ] {
            let values = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert_eq!(parse(&values).err(), Some(CredentialFailure::usage()));
        }
    }

    #[test]
    fn import_writes_once_and_never_overwrites_an_existing_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let input = root.path().join("backup.key");
        fs::write(&input, key())?;
        let command = ImportCommand {
            credential_id: CredentialId::parse("credential-001")?,
            input,
        };
        let store = Store::default();
        execute(command, &store).map_err(|_| std::io::Error::other("key import failed"))?;
        let repeat = ImportCommand {
            credential_id: CredentialId::parse("credential-001")?,
            input: root.path().join("backup.key"),
        };
        assert_eq!(
            execute(repeat, &store).err(),
            Some(CredentialFailure::already_exists())
        );
        Ok(())
    }

    #[test]
    fn concurrent_imports_never_overwrite_the_same_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let input = root.path().join("backup.key");
        fs::write(&input, key())?;
        let store = Arc::new(Store::default());
        let lock_directory = root.path().join("locks");
        let results = std::thread::scope(|scope| -> Result<_, CredentialFailure> {
            let first_store = Arc::clone(&store);
            let second_store = Arc::clone(&store);
            let first_input = input.clone();
            let second_input = input.clone();
            let first_locks = lock_directory.clone();
            let second_locks = lock_directory.clone();
            let first = scope.spawn(move || import(first_input, first_locks, &first_store));
            let second = scope.spawn(move || import(second_input, second_locks, &second_store));
            Ok([
                first.join().map_err(|_| CredentialFailure::store())?,
                second.join().map_err(|_| CredentialFailure::store())?,
            ])
        })
        .map_err(|_| std::io::Error::other("credential import thread failed"))?;
        let successful = results.iter().filter(|result| result.is_ok()).count();
        let rejected = results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(CredentialFailure {
                        code: "credential_already_exists",
                        ..
                    })
                )
            })
            .count();
        assert_eq!(successful, 1);
        assert_eq!(rejected, 1);
        assert_eq!(store.writes.load(Ordering::Relaxed), 1);
        Ok(())
    }

    fn import(input: PathBuf, locks: PathBuf, store: &Store) -> Result<(), CredentialFailure> {
        execute_with_lock_directory(
            ImportCommand {
                credential_id: CredentialId::parse("credential-race")
                    .map_err(|_| CredentialFailure::usage())?,
                input,
            },
            store,
            &locks,
        )
        .map(|_| ())
    }

    fn key() -> Vec<u8> {
        let mut payload = b"openssh-key-v1\0".to_vec();
        for value in [b"none".as_slice(), b"none", b""] {
            payload.extend_from_slice(&(value.len() as u32).to_be_bytes());
            payload.extend_from_slice(value);
        }
        let encoded = STANDARD.encode(payload);
        format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{encoded}\n-----END OPENSSH PRIVATE KEY-----\n"
        )
        .into_bytes()
    }

    #[derive(Default)]
    struct Store {
        values: Mutex<HashMap<String, Vec<u8>>>,
        writes: AtomicUsize,
    }

    impl SecretStore for Store {
        fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            let values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            Ok(values.get(id.as_str()).cloned().map(SecretValue::new))
        }

        fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.insert(id.as_str().to_owned(), secret.expose().to_vec());
            self.writes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
}
