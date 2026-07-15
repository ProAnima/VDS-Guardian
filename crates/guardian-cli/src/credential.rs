use crate::secret_store::resolve_store;
use fs2::FileExt;
use guardian_core::{CredentialId, SecretStore, SecretValue};
use guardian_ssh::SshIdentity;
use serde::Serialize;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::ExitCode,
};

const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;
const MAX_PUBLIC_KEY_FILE_BYTES: u64 = 16 * 1024;

pub(super) fn run(arguments: &[OsString]) -> ExitCode {
    match parse(arguments).and_then(|command| {
        let store = resolve_store(command.vault_dir()).map_err(|_| CredentialFailure::store())?;
        execute(command, &store)
    }) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<Command, CredentialFailure> {
    match arguments.first().and_then(|value| value.to_str()) {
        Some("import-ssh-key") => parse_import(arguments).map(Command::Import),
        Some("register-agent-key") => {
            parse_register_agent_key(arguments).map(Command::RegisterAgentKey)
        }
        _ => Err(CredentialFailure::usage()),
    }
}

fn parse_import(arguments: &[OsString]) -> Result<ImportCommand, CredentialFailure> {
    let mut credential_id = None;
    let mut input = None;
    let mut vault_dir = None;
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
            Some("--vault-dir") => {
                index += 1;
                vault_dir = arguments.get(index).map(PathBuf::from);
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
    if !json || !input.is_absolute() || vault_dir.as_deref().is_some_and(|dir| !dir.is_absolute()) {
        return Err(CredentialFailure::usage());
    }
    Ok(ImportCommand {
        credential_id,
        input,
        vault_dir,
    })
}

fn parse_register_agent_key(
    arguments: &[OsString],
) -> Result<RegisterAgentKeyCommand, CredentialFailure> {
    let mut credential_id = None;
    let mut public_key_file = None;
    let mut vault_dir = None;
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--credential-id") => {
                index += 1;
                credential_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--public-key-file") => {
                index += 1;
                public_key_file = arguments.get(index).map(PathBuf::from);
            }
            Some("--vault-dir") => {
                index += 1;
                vault_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--json") => json = true,
            _ => return Err(CredentialFailure::usage()),
        }
        index += 1;
    }
    let credential_id = credential_id
        .ok_or_else(CredentialFailure::usage)
        .and_then(|value| CredentialId::parse(value).map_err(|_| CredentialFailure::usage()))?;
    let public_key_file = public_key_file.ok_or_else(CredentialFailure::usage)?;
    if !json
        || !public_key_file.is_absolute()
        || vault_dir.as_deref().is_some_and(|dir| !dir.is_absolute())
    {
        return Err(CredentialFailure::usage());
    }
    Ok(RegisterAgentKeyCommand {
        credential_id,
        public_key_file,
        vault_dir,
    })
}

fn execute(
    command: Command,
    store: &dyn SecretStore,
) -> Result<CredentialOutput, CredentialFailure> {
    execute_with_lock_directory(command, store, &credential_lock_directory()?)
}

fn execute_with_lock_directory(
    command: Command,
    store: &dyn SecretStore,
    lock_directory: &Path,
) -> Result<CredentialOutput, CredentialFailure> {
    let secret = resolve_secret(&command)?;
    let credential_id = command.credential_id().clone();
    let _lock = credential_lock(lock_directory, &credential_id)?;
    if store
        .load(&credential_id)
        .map_err(|_| CredentialFailure::store())?
        .is_some()
    {
        return Err(CredentialFailure::already_exists());
    }
    store
        .store(&credential_id, &secret)
        .map_err(|_| CredentialFailure::store())?;
    let stored = store
        .load(&credential_id)
        .map_err(|_| CredentialFailure::store())?
        .ok_or_else(CredentialFailure::store)?;
    SshIdentity::validate(stored.expose()).map_err(|_| CredentialFailure::store())?;
    Ok(CredentialOutput { credential_id })
}

/// Resolves either subcommand down to the bytes that get stored under the
/// credential ID: a real private key's raw bytes for `import-ssh-key`, or
/// an `AGENT-IDENTITY-V1` marker (never a secret by itself, only a public
/// key) for `register-agent-key`. Both are validated the same way on
/// read-back in `execute_with_lock_directory` since `SshIdentity::validate`
/// classifies either shape.
fn resolve_secret(command: &Command) -> Result<SecretValue, CredentialFailure> {
    match command {
        Command::Import(command) => {
            let key = read_key(&command.input)?;
            SshIdentity::validate(key.expose()).map_err(|_| CredentialFailure::invalid_key())?;
            Ok(key)
        }
        Command::RegisterAgentKey(command) => {
            let (algorithm, public_key_base64) = read_public_key_file(&command.public_key_file)?;
            let marker = SshIdentity::encode_agent_identity(&algorithm, &public_key_base64)
                .map_err(|_| CredentialFailure::invalid_public_key())?;
            Ok(SecretValue::new(marker))
        }
    }
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
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_KEY_FILE_BYTES
    {
        return Err(CredentialFailure::input());
    }
    fs::read(path)
        .map(SecretValue::new)
        .map_err(|_| CredentialFailure::input())
}

/// Reads a standard OpenSSH `.pub` file (`"<algorithm> <base64> [comment]"`
/// — the exact text `ssh-keygen` itself produces) and returns just the
/// algorithm and base64 fields; any trailing comment is ignored, matching
/// how OpenSSH itself treats that field as free-form. Real validation of
/// the algorithm/blob happens in `SshIdentity::encode_agent_identity`.
fn read_public_key_file(path: &Path) -> Result<(String, String), CredentialFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CredentialFailure::public_key_input())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PUBLIC_KEY_FILE_BYTES
    {
        return Err(CredentialFailure::public_key_input());
    }
    let bytes = fs::read(path).map_err(|_| CredentialFailure::public_key_input())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CredentialFailure::invalid_public_key())?;
    let mut fields = text.trim().split_ascii_whitespace();
    let algorithm = fields
        .next()
        .ok_or_else(CredentialFailure::invalid_public_key)?;
    let public_key_base64 = fields
        .next()
        .ok_or_else(CredentialFailure::invalid_public_key)?;
    Ok((algorithm.to_owned(), public_key_base64.to_owned()))
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

enum Command {
    Import(ImportCommand),
    RegisterAgentKey(RegisterAgentKeyCommand),
}

impl Command {
    fn vault_dir(&self) -> Option<&Path> {
        match self {
            Self::Import(command) => command.vault_dir.as_deref(),
            Self::RegisterAgentKey(command) => command.vault_dir.as_deref(),
        }
    }

    fn credential_id(&self) -> &CredentialId {
        match self {
            Self::Import(command) => &command.credential_id,
            Self::RegisterAgentKey(command) => &command.credential_id,
        }
    }
}

struct ImportCommand {
    credential_id: CredentialId,
    input: PathBuf,
    vault_dir: Option<PathBuf>,
}

struct RegisterAgentKeyCommand {
    credential_id: CredentialId,
    public_key_file: PathBuf,
    vault_dir: Option<PathBuf>,
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
            usage: "guardian-cli credential import-ssh-key --credential-id <id> --input <absolute-key-path> [--vault-dir <absolute-path>] --json | guardian-cli credential register-agent-key --credential-id <id> --public-key-file <absolute-path> [--vault-dir <absolute-path>] --json",
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

    fn public_key_input() -> Self {
        Self {
            code: "invalid_public_key_input",
            message: "The public key input is not a safe regular file.",
            usage: "Provide an absolute non-symlink .pub file no larger than 16 KiB.",
        }
    }

    fn invalid_public_key() -> Self {
        Self {
            code: "invalid_public_key",
            message: "The public key is not a supported SSH public key.",
            usage: "Provide an ssh-ed25519 or ecdsa-sha2-nistp256/384/521 public key file, such as the .pub file OpenSSH itself produces. The matching private key must already be loaded in an OS SSH agent at connection time.",
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
    use super::{
        Command, CredentialFailure, ImportCommand, RegisterAgentKeyCommand, execute,
        execute_with_lock_directory, parse,
    };
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
    fn a_relative_vault_dir_is_rejected() {
        let values = [
            "import-ssh-key",
            "--credential-id",
            "credential-001",
            "--input",
            "/key",
            "--vault-dir",
            "relative",
            "--json",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        assert_eq!(parse(&values).err(), Some(CredentialFailure::usage()));
    }

    #[test]
    fn register_agent_key_requires_explicit_json_and_absolute_public_key_path() {
        for arguments in [
            vec!["register-agent-key"],
            vec![
                "register-agent-key",
                "--credential-id",
                "id",
                "--public-key-file",
                "key.pub",
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
    fn an_unrecognized_subcommand_is_rejected() {
        let values = ["rotate-ssh-key"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(parse(&values).err(), Some(CredentialFailure::usage()));
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
            vault_dir: None,
        };
        let store = Store::default();
        execute(Command::Import(command), &store)
            .map_err(|_| std::io::Error::other("key import failed"))?;
        let repeat = ImportCommand {
            credential_id: CredentialId::parse("credential-001")?,
            input: root.path().join("backup.key"),
            vault_dir: None,
        };
        assert_eq!(
            execute(Command::Import(repeat), &store).err(),
            Some(CredentialFailure::already_exists())
        );
        Ok(())
    }

    #[test]
    fn register_agent_key_writes_once_and_never_overwrites_an_existing_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let public_key_file = root.path().join("agent.pub");
        fs::write(&public_key_file, public_key_line())?;
        let command = RegisterAgentKeyCommand {
            credential_id: CredentialId::parse("credential-agent")?,
            public_key_file: public_key_file.clone(),
            vault_dir: None,
        };
        let store = Store::default();
        execute(Command::RegisterAgentKey(command), &store)
            .map_err(|_| std::io::Error::other("agent key registration failed"))?;
        let repeat = RegisterAgentKeyCommand {
            credential_id: CredentialId::parse("credential-agent")?,
            public_key_file,
            vault_dir: None,
        };
        assert_eq!(
            execute(Command::RegisterAgentKey(repeat), &store).err(),
            Some(CredentialFailure::already_exists())
        );
        Ok(())
    }

    #[test]
    fn register_agent_key_rejects_a_private_key_fed_in_as_a_public_key_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let public_key_file = root.path().join("not-a-pub-file");
        fs::write(&public_key_file, key())?;
        let command = RegisterAgentKeyCommand {
            credential_id: CredentialId::parse("credential-agent")?,
            public_key_file,
            vault_dir: None,
        };
        let store = Store::default();
        assert_eq!(
            execute(Command::RegisterAgentKey(command), &store).err(),
            Some(CredentialFailure::invalid_public_key())
        );
        Ok(())
    }

    #[test]
    fn register_agent_key_rejects_a_disallowed_algorithm() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let public_key_file = root.path().join("agent.pub");
        fs::write(
            &public_key_file,
            format!("ssh-rsa {} comment\n", rsa_shaped_blob()),
        )?;
        let command = RegisterAgentKeyCommand {
            credential_id: CredentialId::parse("credential-agent")?,
            public_key_file,
            vault_dir: None,
        };
        let store = Store::default();
        assert_eq!(
            execute(Command::RegisterAgentKey(command), &store).err(),
            Some(CredentialFailure::invalid_public_key())
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
            Command::Import(ImportCommand {
                credential_id: CredentialId::parse("credential-race")
                    .map_err(|_| CredentialFailure::usage())?,
                input,
                vault_dir: None,
            }),
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

    fn public_key_line() -> String {
        let mut payload = Vec::new();
        payload.extend_from_slice(&11_u32.to_be_bytes());
        payload.extend_from_slice(b"ssh-ed25519");
        payload.push(1);
        format!("ssh-ed25519 {} comment\n", STANDARD.encode(payload))
    }

    fn rsa_shaped_blob() -> String {
        let mut payload = Vec::new();
        payload.extend_from_slice(&7_u32.to_be_bytes());
        payload.extend_from_slice(b"ssh-rsa");
        payload.push(1);
        STANDARD.encode(payload)
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

        fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.remove(id.as_str());
            Ok(())
        }
    }
}
