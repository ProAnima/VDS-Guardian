//! Streaming validation and extraction for tar.zst archives and single-file
//! zstd payloads (for example a database snapshot).

mod cancellation;
mod writer;
mod zstd_file;

use cancellation::{CancellationReader, check as check_cancellation, map_read};
use guardian_core::{
    ArchiveInspectionPort, ArchiveInspectionPortError, ArchivePath, CancellationHandle,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::Path,
};
use tar::Archive;
use thiserror::Error;

pub use writer::{ArchiveWriteError, TarZstdWriter};
pub use zstd_file::{
    ZstdFileInspector, decompress_zstd_file, decompress_zstd_file_with_cancellation,
    inspect_zstd_file,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    Directory,
    RegularFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntrySummary {
    pub path: String,
    pub kind: ArchiveEntryKind,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntryPage {
    pub entries: Vec<ArchiveEntrySummary>,
    pub total_entries: u64,
    pub next_offset: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("archive operation was cancelled")]
    Cancelled,
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

pub fn list_tar_zstd_entries(
    source: impl Read,
    limits: ArchiveLimits,
    offset: u64,
    limit: u64,
) -> Result<ArchiveEntryPage, ArchiveError> {
    if limit == 0 || limit > 500 || offset > limits.max_entries {
        return Err(ArchiveError::Invalid);
    }
    let reader = bounded_zstd_reader(source, limits.max_expanded_bytes)?;
    list_tar_entries(reader, limits, offset, limit)
}

/// Extracts a validated archive into a newly created empty directory.
/// Existing destinations are rejected so extraction can never merge with a live target.
pub fn extract_tar_zstd(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveInspection, ArchiveError> {
    extract_tar_zstd_inner(source, destination, limits, None)
}

pub fn extract_tar_zstd_with_cancellation(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
    cancellation: &CancellationHandle,
) -> Result<ArchiveInspection, ArchiveError> {
    extract_tar_zstd_inner(source, destination, limits, Some(cancellation))
}

fn extract_tar_zstd_inner(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
    cancellation: Option<&CancellationHandle>,
) -> Result<ArchiveInspection, ArchiveError> {
    check_cancellation(cancellation)?;
    fs::create_dir(destination).map_err(|_| ArchiveError::Invalid)?;
    let result = extract_new_tar_zstd(source, destination, limits, cancellation);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn extract_new_tar_zstd(
    source: impl Read,
    destination: &Path,
    limits: ArchiveLimits,
    cancellation: Option<&CancellationHandle>,
) -> Result<ArchiveInspection, ArchiveError> {
    let mut source = bounded_zstd_reader_inner(source, limits.max_expanded_bytes, cancellation)?;
    let mut inspection = ArchiveInspection {
        entries: 0,
        regular_files: 0,
        directories: 0,
        expanded_bytes: 0,
    };
    {
        let mut archive = Archive::new(&mut source);
        for entry in archive
            .entries()
            .map_err(|error| map_read(error, cancellation))?
        {
            check_cancellation(cancellation)?;
            extract_entry(
                entry.map_err(|error| map_read(error, cancellation))?,
                destination,
                limits,
                &mut inspection,
                cancellation,
            )?;
        }
    }
    drain_expanded_stream(&mut source, cancellation)?;
    inspection.expanded_bytes = source.consumed;
    Ok(inspection)
}

fn extract_entry<R: Read>(
    mut entry: tar::Entry<'_, R>,
    destination: &Path,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
    cancellation: Option<&CancellationHandle>,
) -> Result<(), ArchiveError> {
    check_cancellation(cancellation)?;
    let header = entry.header();
    let raw_path = entry.path().map_err(|_| ArchiveError::UnsafePath)?;
    let path = parse_entry_path(&raw_path, header.entry_type().is_dir())?;
    inspection.entries = inspection
        .entries
        .checked_add(1)
        .ok_or(ArchiveError::Invalid)?;
    if inspection.entries > limits.max_entries {
        return Err(ArchiveError::Invalid);
    }
    let output = destination.join(path.as_str());
    let parent = output.parent().ok_or(ArchiveError::UnsafePath)?;
    ensure_parent_directories(destination, parent)?;
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
    let copied =
        io::copy(&mut entry, &mut output_file).map_err(|error| map_read(error, cancellation))?;
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

/// Real tar output only ever includes entries for the capture root itself
/// and its descendants — never separate entries for path segments *above*
/// a multi-segment root (capturing `/srv/app` yields an entry named
/// `srv/app`, never a lone `srv` entry first). Creating any missing
/// ancestor here — hardened exactly like every other directory this
/// extractor creates — is safe: `parent` is always `destination` joined
/// with a prefix of an already-`ArchivePath`-validated relative path (no
/// `..`, no absolute segments, no symlink entries ever extracted by this
/// code), so it can never resolve outside `destination`.
fn ensure_parent_directories(destination: &Path, parent: &Path) -> Result<(), ArchiveError> {
    let mut missing = Vec::new();
    let mut current = parent;
    while current != destination {
        if current.is_dir() {
            break;
        }
        missing.push(current);
        current = current.parent().ok_or(ArchiveError::UnsafePath)?;
    }
    for directory in missing.into_iter().rev() {
        fs::create_dir(directory).map_err(|_| ArchiveError::Invalid)?;
        restrict_directory(directory)?;
    }
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
    drain_expanded_stream(&mut source, None)?;
    inspection.expanded_bytes = source.consumed;
    Ok(inspection)
}

fn list_tar_entries<R: Read>(
    source: R,
    limits: ArchiveLimits,
    offset: u64,
    limit: u64,
) -> Result<ArchiveEntryPage, ArchiveError> {
    let mut source = ReadBudget::new(source, limits.max_expanded_bytes);
    let mut inspection = ArchiveInspection {
        entries: 0,
        regular_files: 0,
        directories: 0,
        expanded_bytes: 0,
    };
    let mut selected = Vec::new();
    {
        let mut archive = Archive::new(&mut source);
        for entry in archive.entries().map_err(|_| ArchiveError::Invalid)? {
            let entry = entry.map_err(|_| ArchiveError::Invalid)?;
            let summary = summarize_entry(&entry, limits, &mut inspection)?;
            if inspection.entries > offset && selected.len() < limit as usize {
                selected.push(summary);
            }
        }
    }
    drain_expanded_stream(&mut source, None)?;
    let returned_through = offset.saturating_add(selected.len() as u64);
    Ok(ArchiveEntryPage {
        entries: selected,
        total_entries: inspection.entries,
        next_offset: (returned_through < inspection.entries).then_some(returned_through),
    })
}

fn summarize_entry<R: Read>(
    entry: &tar::Entry<'_, R>,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
) -> Result<ArchiveEntrySummary, ArchiveError> {
    let header = entry.header();
    let path = parse_entry_path(
        &entry.path().map_err(|_| ArchiveError::UnsafePath)?,
        header.entry_type().is_dir(),
    )?;
    inspect_entry_header(header, limits, inspection)?;
    let (kind, size) = if header.entry_type().is_dir() {
        (ArchiveEntryKind::Directory, 0)
    } else {
        (
            ArchiveEntryKind::RegularFile,
            header.size().map_err(|_| ArchiveError::Invalid)?,
        )
    };
    Ok(ArchiveEntrySummary {
        path: path.as_str().to_owned(),
        kind,
        size,
    })
}

pub(crate) fn drain_expanded_stream(
    source: &mut impl Read,
    cancellation: Option<&CancellationHandle>,
) -> Result<(), ArchiveError> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancellation(cancellation)?;
        if source
            .read(&mut buffer)
            .map_err(|error| map_read(error, cancellation))?
            == 0
        {
            return Ok(());
        }
    }
}

/// Real tar writers (GNU tar, BSD tar, and every other implementation this
/// project's own remote capture command relies on) always suffix a
/// directory member's own name with `/` — the entry type byte already says
/// it's a directory, so the trailing slash is pure wire-format convention,
/// not an extra path segment. Strip exactly one before validating, but only
/// for directory entries: a *file* entry ending in `/` is not real tar
/// output and stays rejected. This project's own `TarZstdWriter` never
/// produces this shape (`ArchivePath` itself cannot hold a trailing slash),
/// which is why no existing test archive ever exercised this path before a
/// real `tar`-produced archive did.
fn parse_entry_path(path: &Path, is_directory: bool) -> Result<ArchivePath, ArchiveError> {
    let raw = path.to_string_lossy();
    let candidate = if is_directory {
        raw.strip_suffix('/').unwrap_or(&raw)
    } else {
        raw.as_ref()
    };
    ArchivePath::parse(candidate).map_err(|_| ArchiveError::UnsafePath)
}

fn inspect_entry<R: Read>(
    entry: tar::Entry<'_, R>,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
) -> Result<(), ArchiveError> {
    let header = entry.header();
    let raw_path = entry.path().map_err(|_| ArchiveError::UnsafePath)?;
    parse_entry_path(&raw_path, header.entry_type().is_dir())?;
    inspect_entry_header(header, limits, inspection)
}

fn inspect_entry_header(
    header: &tar::Header,
    limits: ArchiveLimits,
    inspection: &mut ArchiveInspection,
) -> Result<(), ArchiveError> {
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
    bounded_zstd_reader_inner(source, max_bytes, None)
}

pub(crate) fn bounded_zstd_reader_inner(
    source: impl Read,
    max_bytes: u64,
    cancellation: Option<&CancellationHandle>,
) -> Result<ReadBudget<impl Read>, ArchiveError> {
    check_cancellation(cancellation)?;
    let source = CancellationReader::new(source, cancellation);
    let decoder =
        zstd::stream::read::Decoder::new(source).map_err(|error| map_read(error, cancellation))?;
    Ok(ReadBudget::new(
        CancellationReader::new(decoder, cancellation),
        max_bytes,
    ))
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
