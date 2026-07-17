//! Validation and extraction for a single file compressed with zstd (for
//! example a database snapshot), as opposed to a tar.zst archive.

use crate::cancellation::{check as check_cancellation, map_read};
use crate::{ArchiveError, bounded_zstd_reader, bounded_zstd_reader_inner, drain_expanded_stream};
use guardian_core::{ArchiveInspectionPort, ArchiveInspectionPortError, CancellationHandle};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::Path,
};

/// Decompresses a bounded zstd stream that is a single file, not a tar
/// archive, into exactly one destination file. The destination must not
/// already exist, and is removed again if decompression fails partway.
pub fn decompress_zstd_file(
    source: impl Read,
    destination: &Path,
    max_bytes: u64,
) -> Result<u64, ArchiveError> {
    decompress_zstd_file_inner(source, destination, max_bytes, None)
}

pub fn decompress_zstd_file_with_cancellation(
    source: impl Read,
    destination: &Path,
    max_bytes: u64,
    cancellation: &CancellationHandle,
) -> Result<u64, ArchiveError> {
    decompress_zstd_file_inner(source, destination, max_bytes, Some(cancellation))
}

fn decompress_zstd_file_inner(
    source: impl Read,
    destination: &Path,
    max_bytes: u64,
    cancellation: Option<&CancellationHandle>,
) -> Result<u64, ArchiveError> {
    check_cancellation(cancellation)?;
    let result = decompress_new_zstd_file(source, destination, max_bytes, cancellation);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn decompress_new_zstd_file(
    source: impl Read,
    destination: &Path,
    max_bytes: u64,
    cancellation: Option<&CancellationHandle>,
) -> Result<u64, ArchiveError> {
    let mut reader = bounded_zstd_reader_inner(source, max_bytes, cancellation)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| ArchiveError::Invalid)?;
    io::copy(&mut reader, &mut output).map_err(|error| map_read(error, cancellation))?;
    output.sync_all().map_err(|_| ArchiveError::Invalid)?;
    Ok(reader.consumed)
}

/// Validates that a payload is a well-formed, bounded zstd stream without
/// writing its decompressed content anywhere (the capture-time counterpart
/// of [`decompress_zstd_file`], mirroring how `inspect_tar_zstd` validates
/// before `extract_tar_zstd` extracts).
pub fn inspect_zstd_file(source: impl Read, max_bytes: u64) -> Result<u64, ArchiveError> {
    let mut reader = bounded_zstd_reader(source, max_bytes)?;
    drain_expanded_stream(&mut reader, None)?;
    Ok(reader.consumed)
}

/// [`ArchiveInspectionPort`] for a single-file zstd payload, as opposed to
/// `TarZstdInspector`'s tar.zst archives.
pub struct ZstdFileInspector {
    max_bytes: u64,
}

impl ZstdFileInspector {
    #[must_use]
    pub const fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }
}

impl ArchiveInspectionPort for ZstdFileInspector {
    fn inspect(&self, payload: &Path) -> Result<(), ArchiveInspectionPortError> {
        let file = File::open(payload).map_err(|_| ArchiveInspectionPortError::Rejected)?;
        inspect_zstd_file(file, self.max_bytes)
            .map(|_| ())
            .map_err(|_| ArchiveInspectionPortError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ZstdFileInspector, decompress_zstd_file, decompress_zstd_file_with_cancellation,
        inspect_zstd_file,
    };
    use guardian_core::{ArchiveInspectionPort, CancellationHandle};
    use std::io::{self, Cursor, Read};

    #[test]
    fn decompress_zstd_file_reconstructs_the_original_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let original = b"a lightweight embedded database snapshot".repeat(64);
        let compressed = zstd::stream::encode_all(Cursor::new(&original[..]), 0)?;
        let dir = tempfile::tempdir()?;
        let destination = dir.path().join("database.sqlite");
        let consumed = decompress_zstd_file(
            Cursor::new(compressed),
            &destination,
            u64::try_from(original.len())?,
        )?;
        assert_eq!(consumed, u64::try_from(original.len())?);
        assert_eq!(std::fs::read(&destination)?, original);
        Ok(())
    }

    #[test]
    fn decompress_zstd_file_rejects_a_stream_over_budget() -> Result<(), Box<dyn std::error::Error>>
    {
        let original = b"a lightweight embedded database snapshot".repeat(64);
        let compressed = zstd::stream::encode_all(Cursor::new(&original[..]), 0)?;
        let dir = tempfile::tempdir()?;
        let destination = dir.path().join("database.sqlite");
        assert!(decompress_zstd_file(Cursor::new(compressed), &destination, 8).is_err());
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    fn inspector_accepts_a_valid_stream_within_budget() -> Result<(), Box<dyn std::error::Error>> {
        let original = b"snapshot bytes";
        let compressed = zstd::stream::encode_all(Cursor::new(&original[..]), 0)?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("payload.zst");
        std::fs::write(&path, compressed)?;
        ZstdFileInspector::new(1024).inspect(&path)?;
        Ok(())
    }

    #[test]
    fn inspector_rejects_a_stream_over_budget() -> Result<(), Box<dyn std::error::Error>> {
        let original = b"snapshot bytes".repeat(1024);
        let compressed = zstd::stream::encode_all(Cursor::new(&original[..]), 0)?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("payload.zst");
        std::fs::write(&path, compressed)?;
        assert!(ZstdFileInspector::new(8).inspect(&path).is_err());
        Ok(())
    }

    #[test]
    fn inspect_zstd_file_reports_the_expanded_byte_count() -> Result<(), Box<dyn std::error::Error>>
    {
        let original = b"count these bytes exactly".to_vec();
        let compressed = zstd::stream::encode_all(Cursor::new(&original[..]), 0)?;
        let consumed = inspect_zstd_file(Cursor::new(compressed), u64::try_from(original.len())?)?;
        assert_eq!(consumed, u64::try_from(original.len())?);
        Ok(())
    }

    #[test]
    fn cancelled_decompression_removes_the_partial_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let original = (0..1_000_000)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let compressed = zstd::stream::encode_all(Cursor::new(original), 0)?;
        let handle = CancellationHandle::new();
        let source = CancelAfterReads::new(Cursor::new(compressed), handle.clone(), 100);
        let dir = tempfile::tempdir()?;
        let destination = dir.path().join("database.sqlite");
        assert!(matches!(
            decompress_zstd_file_with_cancellation(source, &destination, 2_000_000, &handle,),
            Err(crate::ArchiveError::Cancelled)
        ));
        assert!(!destination.exists());
        Ok(())
    }

    struct CancelAfterReads<R> {
        inner: R,
        handle: CancellationHandle,
        remaining: usize,
    }

    impl<R> CancelAfterReads<R> {
        fn new(inner: R, handle: CancellationHandle, remaining: usize) -> Self {
            Self {
                inner,
                handle,
                remaining,
            }
        }
    }

    impl<R: Read> Read for CancelAfterReads<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                self.handle.cancel();
            } else {
                self.remaining -= 1;
            }
            let maximum = buffer.len().min(1);
            self.inner.read(&mut buffer[..maximum])
        }
    }
}
