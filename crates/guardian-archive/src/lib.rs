//! Streaming validation and extraction for tar.zst archives and single-file
//! zstd payloads (for example a database snapshot).

mod writer;
mod zstd_file;

use guardian_core::{ArchiveInspectionPort, ArchiveInspectionPortError, ArchivePath};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::Path,
};
use tar::Archive;
use thiserror::Error;

pub use writer::{ArchiveWriteError, TarZstdWriter};
pub use zstd_file::{ZstdFileInspector, decompress_zstd_file, inspect_zstd_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_entries: u64,
    pub max_file_bytes: u64,
    pub max_expanded_bytes: u64,
}

impl ArchiveLimits {
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            max_entries: 100_000,
            max_file_bytes: 16 * 1024 * 1024 * 1024,
            max_expanded_bytes: 256 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveInspection {
    pub entries: u64,
    pub regular_files: u64,
    pub directories: u64,
    pub expanded_bytes: u64,
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("archive is malformed or exceeds a resource limit")]
    Invalid,
    #[error("archive entry path is unsafe")]
    UnsafePath,
    #[error("archive entry type is not supported")]
    UnsupportedEntryType,
}

pub struct TarZstdInspector {
    limits: ArchiveLimits,
}

impl TarZstdInspector {
    #[must_use]
    pub const fn new(limits: ArchiveLimits) -> Self {
        Self { limits }
    }
}

impl ArchiveInspectionPort for TarZstdInspector {
    fn inspect(&self, payload: &Path) -> Result<(), ArchiveInspectionPortError> {
        let file = File::open(payload).map_err(|_| ArchiveInspectionPortError::Rejected)?;
        inspect_tar_zstd(file, self.limits)
            .map(|_| ())
            .map_err(|_| ArchiveInspectionPortError::Rejected)
    }
}

pub fn inspect_tar_zstd(
    source: impl Read,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    inspect_tar(
        bounded_zstd_reader(source, limits.max_expanded_bytes)?,
        limits,
    )
}

/// Extracts a validated archive into a newly created empty directory.
/// Existing destinations are rejected so extraction can never merge with a live target.
pub fn extract_tar_zstd(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    fs::create_dir(destination).map_err(|_| ArchiveError::Invalid)?;
    let result = extract_new_tar_zstd(source, destination, limits);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn extract_new_tar_zstd(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    let mut source = bounded_zstd_reader(source, limits.max_expanded_bytes)?;
    let mut inspection = ArchiveInspection {
        entries: 0,
        regular_files: 0,
        directories: 0,
        expanded_bytes: 0,
    };
    {
        let mut archive = Archive::new(&mut source);
        for entry in archive.entries().map_err(|_| ArchiveError::Invalid)? {
            extract_entry(
                entry.map_err(|_| ArchiveError::Invalid)?,
                destination,
                limits,
                &mut inspection,
            )?;
        }
    }
    drain_expanded_stream(&mut source)?;
    inspection.expanded_bytes = source.consumed;
    Ok(inspection)
}

fn extract_entry<R: Read>(
    mut entry: tar::Entry<'_, R>,
    destination: &Path,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
) -> Result<(), ArchiveError> {
    let header = entry.header();
    let path = entry.path().map_err(|_| ArchiveError::UnsafePath)?;
    let path = ArchivePath::parse(path.to_string_lossy().into_owned())
        .map_err(|_| ArchiveError::UnsafePath)?;
    inspection.entries = inspection
        .entries
        .checked_add(1)
        .ok_or(ArchiveError::Invalid)?;
    if inspection.entries > limits.max_entries {
        return Err(ArchiveError::Invalid);
    }
    let output = destination.join(path.as_str());
    let parent = output.parent().ok_or(ArchiveError::UnsafePath)?;
    if !parent.is_dir() {
        return Err(ArchiveError::UnsafePath);
    }
    if header.entry_type().is_dir() {
        fs::create_dir(&output).map_err(|_| ArchiveError::Invalid)?;
        restrict_directory(&output)?;
        inspection.directories = inspection
            .directories
            .checked_add(1)
            .ok_or(ArchiveError::Invalid)?;
        return Ok(());
    }
    if !header.entry_type().is_file() {
        return Err(ArchiveError::UnsupportedEntryType);
    }
    let size = header.size().map_err(|_| ArchiveError::Invalid)?;
    if size > limits.max_file_bytes {
        return Err(ArchiveError::Invalid);
    }
    let mut output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|_| ArchiveError::Invalid)?;
    let copied = io::copy(&mut entry, &mut output_file).map_err(|_| ArchiveError::Invalid)?;
    output_file.sync_all().map_err(|_| ArchiveError::Invalid)?;
    if copied != size {
        return Err(ArchiveError::Invalid);
    }
    restrict_file(&output)?;
    inspection.regular_files = inspection
        .regular_files
        .checked_add(1)
        .ok_or(ArchiveError::Invalid)?;
    Ok(())
}

fn restrict_file(path: &Path) -> Result<(), ArchiveError> {
    let _ = path;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|_| ArchiveError::Invalid)?;
    Ok(())
}

fn restrict_directory(path: &Path) -> Result<(), ArchiveError> {
    let _ = path;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|_| ArchiveError::Invalid)?;
    Ok(())
}

fn inspect_tar<R: Read>(
    source: R,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    let mut source = ReadBudget::new(source, limits.max_expanded_bytes);
    let mut inspection = ArchiveInspection {
        entries: 0,
        regular_files: 0,
        directories: 0,
        expanded_bytes: 0,
    };
    {
        let mut archive = Archive::new(&mut source);
        let entries = archive.entries().map_err(|_| ArchiveError::Invalid)?;
        for entry in entries {
            inspect_entry(
                entry.map_err(|_| ArchiveError::Invalid)?,
                limits,
                &mut inspection,
            )?;
        }
    }
    drain_expanded_stream(&mut source)?;
    inspection.expanded_bytes = source.consumed;
    Ok(inspection)
}

pub(crate) fn drain_expanded_stream(source: &mut impl Read) -> Result<(), ArchiveError> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if source
            .read(&mut buffer)
            .map_err(|_| ArchiveError::Invalid)?
            == 0
        {
            return Ok(());
        }
    }
}

fn inspect_entry<R: Read>(
    entry: tar::Entry<'_, R>,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
) -> Result<(), ArchiveError> {
    let header = entry.header();
    let path = entry.path().map_err(|_| ArchiveError::UnsafePath)?;
    ArchivePath::parse(path.to_string_lossy().into_owned())
        .map_err(|_| ArchiveError::UnsafePath)?;
    inspection.entries = inspection
        .entries
        .checked_add(1)
        .ok_or(ArchiveError::Invalid)?;
    if inspection.entries > limits.max_entries {
        return Err(ArchiveError::Invalid);
    }
    if header.entry_type().is_file() {
        let size = header.size().map_err(|_| ArchiveError::Invalid)?;
        if size > limits.max_file_bytes {
            return Err(ArchiveError::Invalid);
        }
        inspection.regular_files = inspection
            .regular_files
            .checked_add(1)
            .ok_or(ArchiveError::Invalid)?;
    } else if header.entry_type().is_dir() {
        inspection.directories = inspection
            .directories
            .checked_add(1)
            .ok_or(ArchiveError::Invalid)?;
    } else {
        return Err(ArchiveError::UnsupportedEntryType);
    }
    Ok(())
}

/// A [`Read`] wrapper that fails once more than `remaining` bytes have been
/// read from it, and records how many bytes were actually consumed.
pub struct ReadBudget<R> {
    inner: R,
    remaining: u64,
    pub consumed: u64,
}

impl<R> ReadBudget<R> {
    #[must_use]
    pub fn new(inner: R, remaining: u64) -> Self {
        Self {
            inner,
            remaining,
            consumed: 0,
        }
    }
}

/// Wraps a byte stream in a zstd decoder bounded to `max_bytes` of expanded
/// output. Shared by tar.zst archive handling and single-file zstd payloads
/// (for example a database snapshot) so both enforce the same expansion cap
/// with one implementation.
pub fn bounded_zstd_reader(
    source: impl Read,
    max_bytes: u64,
) -> Result<ReadBudget<impl Read>, ArchiveError> {
    let decoder = zstd::stream::read::Decoder::new(source).map_err(|_| ArchiveError::Invalid)?;
    Ok(ReadBudget::new(decoder, max_bytes))
}

impl<R: Read> Read for ReadBudget<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        // Request one byte beyond the remaining budget. A stream that ends
        // exactly at the limit then reports genuine EOF on this final read;
        // a stream with anything left beyond the limit yields more than
        // `remaining` bytes here and is rejected below. A plain
        // `remaining == 0` check would instead reject a stream that is
        // exactly at the limit, even though it never actually exceeded it.
        let request = self.remaining.saturating_add(1);
        let maximum = match usize::try_from(request) {
            Ok(value) => value,
            Err(_) => buffer.len(),
        }
        .min(buffer.len());
        let read = self.inner.read(&mut buffer[..maximum])?;
        let read_bytes =
            u64::try_from(read).map_err(|_| io::Error::other("read count overflow"))?;
        if read_bytes > self.remaining {
            return Err(io::Error::other("expanded archive limit"));
        }
        self.remaining -= read_bytes;
        self.consumed += read_bytes;
        Ok(read)
    }
}
