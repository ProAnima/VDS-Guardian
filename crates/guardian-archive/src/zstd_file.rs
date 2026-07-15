//! Validation and extraction for a single file compressed with zstd (for
//! example a database snapshot), as opposed to a tar.zst archive.

use crate::{ArchiveError, bounded_zstd_reader, drain_expanded_stream};
use guardian_core::{ArchiveInspectionPort, ArchiveInspectionPortError};
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
    let result = decompress_new_zstd_file(source, destination, max_bytes);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn decompress_new_zstd_file(
    source: impl Read,
    destination: &Path,
    max_bytes: u64,
) -> Result<u64, ArchiveError> {
    let mut reader = bounded_zstd_reader(source, max_bytes)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| ArchiveError::Invalid)?;
    io::copy(&mut reader, &mut output).map_err(|_| ArchiveError::Invalid)?;
    output.sync_all().map_err(|_| ArchiveError::Invalid)?;
    Ok(reader.consumed)
}

/// Validates that a payload is a well-formed, bounded zstd stream without
/// writing its decompressed content anywhere (the capture-time counterpart
/// of [`decompress_zstd_file`], mirroring how `inspect_tar_zstd` validates
/// before `extract_tar_zstd` extracts).
pub fn inspect_zstd_file(source: impl Read, max_bytes: u64) -> Result<u64, ArchiveError> {
    let mut reader = bounded_zstd_reader(source, max_bytes)?;
    drain_expanded_stream(&mut reader)?;
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
    use super::{ZstdFileInspector, decompress_zstd_file, inspect_zstd_file};
    use guardian_core::ArchiveInspectionPort;
    use std::io::Cursor;

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
}
