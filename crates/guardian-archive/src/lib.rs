//! Streaming tar.zst validation. This crate inspects archives but never extracts them.

mod writer;

use guardian_core::ArchivePath;
use std::io::{self, Read};
use tar::Archive;
use thiserror::Error;

pub use writer::{ArchiveWriteError, TarZstdWriter};

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

pub fn inspect_tar_zstd(
    source: impl Read,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    let decoder = zstd::stream::read::Decoder::new(source).map_err(|_| ArchiveError::Invalid)?;
    inspect_tar(ReadBudget::new(decoder, limits.max_expanded_bytes), limits)
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
    inspection.expanded_bytes = source.consumed;
    Ok(inspection)
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

struct ReadBudget<R> {
    inner: R,
    remaining: u64,
    consumed: u64,
}

impl<R> ReadBudget<R> {
    fn new(inner: R, remaining: u64) -> Self {
        Self {
            inner,
            remaining,
            consumed: 0,
        }
    }
}

impl<R: Read> Read for ReadBudget<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("expanded archive limit"));
        }
        let maximum = match usize::try_from(self.remaining) {
            Ok(value) => value,
            Err(_) => buffer.len(),
        }
        .min(buffer.len());
        let read = self.inner.read(&mut buffer[..maximum])?;
        let consumed = u64::try_from(read).map_err(|_| io::Error::other("read count overflow"))?;
        self.remaining -= consumed;
        self.consumed += consumed;
        Ok(read)
    }
}
