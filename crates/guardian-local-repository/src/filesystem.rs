use crate::RepositoryError;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn create_safe_parent(root: &Path, relative: &str) -> Result<PathBuf, RepositoryError> {
    ensure_directory(root)?;
    let mut current = root.to_path_buf();
    let mut segments = relative.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return Ok(current.join(segment));
        }
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(RepositoryError::UnsafeFilesystemEntry),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|source| RepositoryError::io("create payload directory", source))?;
            }
            Err(source) => return Err(RepositoryError::io("inspect payload directory", source)),
        }
    }
    Err(RepositoryError::UnsafeFilesystemEntry)
}

pub(crate) fn ensure_directory(path: &Path) -> Result<(), RepositoryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| RepositoryError::io("inspect repository directory", source))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(RepositoryError::UnsafeFilesystemEntry)
    }
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), RepositoryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| RepositoryError::io("create new file", source))?;
    file.write_all(bytes)
        .map_err(|source| RepositoryError::io("write new file", source))?;
    file.sync_all()
        .map_err(|source| RepositoryError::io("sync new file", source))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RepositoryError> {
    let file_name = path
        .file_name()
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    let temporary = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    write_new(&temporary, bytes)?;
    fs::rename(&temporary, path)
        .map_err(|source| RepositoryError::io("publish atomic file", source))?;
    sync_parent(path)
}

pub(crate) fn sync_parent(_path: &Path) -> Result<(), RepositoryError> {
    let parent = _path
        .parent()
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| RepositoryError::io("sync parent directory", source))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| RepositoryError::io("sync parent directory", source))?;
    }
    Ok(())
}

pub(crate) fn restrict_to_owner(path: &Path) -> Result<(), RepositoryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|source| RepositoryError::io("restrict temporary file permissions", source))?;
    }
    #[cfg(windows)]
    {
        restrict_windows_to_owner(path)?;
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
fn restrict_windows_to_owner(path: &Path) -> Result<(), RepositoryError> {
    let identity = std::process::Command::new(windows_system32_binary("whoami.exe"))
        .arg("/user")
        .output()
        .map_err(|source| RepositoryError::io("resolve current user identity", source))?;
    if !identity.status.success() {
        return Err(RepositoryError::PermissionHardening);
    }
    // whoami's table header is localized OEM-codepage text on non-English
    // Windows installs and is not valid UTF-8; decode losslessly and search
    // for the SID token, which is always plain ASCII regardless of locale.
    let sid = String::from_utf8_lossy(&identity.stdout)
        .split_ascii_whitespace()
        .find(|part| part.starts_with("S-1-"))
        .map(str::to_owned)
        .ok_or(RepositoryError::PermissionHardening)?;
    let hardened = std::process::Command::new(windows_system32_binary("icacls.exe"))
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("*{sid}:F"))
        .arg("/c")
        .output()
        .map_err(|source| RepositoryError::io("restrict temporary file ACL", source))?;
    hardened
        .status
        .success()
        .then_some(())
        .ok_or(RepositoryError::PermissionHardening)
}

#[cfg(test)]
mod tests {
    use super::restrict_to_owner;

    #[test]
    fn restrict_to_owner_narrows_a_temporary_files_permissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = tempfile::NamedTempFile::new()?;
        restrict_to_owner(file.path())?;
        #[cfg(windows)]
        {
            let output = std::process::Command::new(super::windows_system32_binary("icacls.exe"))
                .arg(file.path())
                .output()?;
            let rendered = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success());
            assert!(!rendered.contains("(I)"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file.path())?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        Ok(())
    }
}
