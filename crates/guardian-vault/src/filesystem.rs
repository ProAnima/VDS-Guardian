use crate::VaultError;
use fs2::FileExt;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static HELD_VAULTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

struct ProcessLock {
    path: PathBuf,
}

pub(crate) struct VaultLock {
    _file: File,
    _process_lock: ProcessLock,
}

pub(crate) fn acquire_lock(vault_dir: &Path) -> Result<VaultLock, VaultError> {
    let process_lock = ProcessLock::acquire(vault_dir)?;
    fs::create_dir_all(vault_dir)
        .map_err(|source| VaultError::io("create vault directory", source))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(vault_dir.join("vault.lock"))
        .map_err(|source| VaultError::io("open vault lock", source))?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(VaultLock {
            _file: file,
            _process_lock: process_lock,
        }),
        Err(error) if is_lock_contention(&error) => Err(VaultError::Busy),
        Err(source) => Err(VaultError::io("lock vault", source)),
    }
}

/// `flock`'s `EWOULDBLOCK` is classified as `ErrorKind::WouldBlock` by Rust's
/// standard library, but the equivalent Windows `LockFileEx` contention
/// (`ERROR_LOCK_VIOLATION`, raw OS error 33) is not — it surfaces as
/// `ErrorKind::Uncategorized`, so it needs an explicit platform check here.
fn is_lock_contention(error: &std::io::Error) -> bool {
    const ERROR_LOCK_VIOLATION: i32 = 33;
    error.kind() == std::io::ErrorKind::WouldBlock
        || (cfg!(windows) && error.raw_os_error() == Some(ERROR_LOCK_VIOLATION))
}

impl ProcessLock {
    fn acquire(path: &Path) -> Result<Self, VaultError> {
        let mut held = registry().lock().map_err(|_| VaultError::Busy)?;
        if !held.insert(path.to_path_buf()) {
            return Err(VaultError::Busy);
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        if let Ok(mut held) = registry().lock() {
            held.remove(&self.path);
        }
    }
}

fn registry() -> &'static Mutex<HashSet<PathBuf>> {
    HELD_VAULTS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn exists(path: &Path) -> Result<bool, VaultError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(VaultError::io("inspect vault file", source)),
    }
}

pub(crate) fn ensure_existing_directory(path: &Path) -> Result<(), VaultError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(VaultError::NotInitialized);
        }
        Err(source) => return Err(VaultError::io("inspect vault directory", source)),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(VaultError::UnsafeFilesystemEntry)
    }
}

pub(crate) fn create_directory(path: &Path) -> Result<(), VaultError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(VaultError::UnsafeFilesystemEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|source| VaultError::io("create vault directory", source)),
        Err(source) => Err(VaultError::io("inspect vault directory", source)),
    }
}

pub(crate) fn read_file(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, VaultError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(VaultError::io("inspect vault file", source)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(VaultError::UnsafeFilesystemEntry);
    }
    if metadata.len() > max_bytes {
        return Err(VaultError::Corrupt);
    }
    fs::read(path)
        .map(Some)
        .map_err(|source| VaultError::io("read vault file", source))
}

/// Writes `bytes` to `path` via a restricted-permission temporary file and an
/// atomic rename, overwriting any existing file at `path` (matching
/// `SecretStore::store`'s overwrite semantics). Callers that must never
/// overwrite an existing secret (the master key) check with [`exists`] first.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let file_name = path.file_name().ok_or(VaultError::UnsafeFilesystemEntry)?;
    let temporary = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    remove_regular_if_present(&temporary)?;
    let mut file = create_restricted(&temporary)?;
    #[cfg(windows)]
    restrict_windows_to_owner(&temporary)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|source| VaultError::io("write vault file", source))?;
    fs::rename(&temporary, path).map_err(|source| VaultError::io("publish vault file", source))?;
    sync_parent(path)
}

#[cfg(unix)]
fn create_restricted(path: &Path) -> Result<File, VaultError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| VaultError::io("create vault file", source))
}

#[cfg(not(unix))]
fn create_restricted(path: &Path) -> Result<File, VaultError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| VaultError::io("create vault file", source))
}

pub(crate) fn remove_regular_if_present(path: &Path) -> Result<(), VaultError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(VaultError::io("inspect vault file", source)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(VaultError::UnsafeFilesystemEntry);
    }
    fs::remove_file(path).map_err(|source| VaultError::io("remove vault file", source))?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<(), VaultError> {
    let parent = path.parent().ok_or(VaultError::UnsafeFilesystemEntry)?;
    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| VaultError::io("sync vault directory", source))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| VaultError::io("sync vault directory", source))?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows_system32_binary(name: &str) -> PathBuf {
    let mut path = PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
    );
    path.push("System32");
    path.push(name);
    path
}

#[cfg(windows)]
fn restrict_windows_to_owner(path: &Path) -> Result<(), VaultError> {
    let identity = std::process::Command::new(windows_system32_binary("whoami.exe"))
        .arg("/user")
        .output()
        .map_err(|source| VaultError::io("resolve current user identity", source))?;
    if !identity.status.success() {
        return Err(VaultError::io(
            "resolve current user identity",
            std::io::Error::other("whoami exited with a failure status"),
        ));
    }
    // whoami's table header is localized OEM-codepage text on non-English
    // Windows installs and is not valid UTF-8; decode losslessly and search
    // for the SID token, which is always plain ASCII regardless of locale.
    let sid = String::from_utf8_lossy(&identity.stdout)
        .split_ascii_whitespace()
        .find(|part| part.starts_with("S-1-"))
        .map(str::to_owned)
        .ok_or_else(|| {
            VaultError::io(
                "resolve current user identity",
                std::io::Error::other("no SID found in whoami output"),
            )
        })?;
    let hardened = std::process::Command::new(windows_system32_binary("icacls.exe"))
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("*{sid}:F"))
        .arg("/c")
        .output()
        .map_err(|source| VaultError::io("restrict vault file ACL", source))?;
    hardened.status.success().then_some(()).ok_or_else(|| {
        VaultError::io(
            "restrict vault file ACL",
            std::io::Error::other("icacls exited with a failure status"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{VaultError, atomic_write, ensure_existing_directory};

    #[test]
    fn atomic_write_produces_an_owner_restricted_file() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("secret.bin");
        atomic_write(&path, b"top-secret")?;
        assert_eq!(std::fs::read(&path)?, b"top-secret");
        #[cfg(windows)]
        {
            let output = std::process::Command::new(super::windows_system32_binary("icacls.exe"))
                .arg(&path)
                .output()?;
            let rendered = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success());
            assert!(!rendered.contains("(I)"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        Ok(())
    }

    #[test]
    fn atomic_write_overwrites_an_existing_file() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("secret.bin");
        atomic_write(&path, b"first")?;
        atomic_write(&path, b"second")?;
        assert_eq!(std::fs::read(&path)?, b"second");
        Ok(())
    }

    #[test]
    fn ensure_existing_directory_fails_closed_on_a_missing_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let missing = root.path().join("does-not-exist");
        assert!(matches!(
            ensure_existing_directory(&missing),
            Err(VaultError::NotInitialized)
        ));
        Ok(())
    }
}
