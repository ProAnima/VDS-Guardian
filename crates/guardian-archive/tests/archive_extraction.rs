use guardian_archive::{ArchiveError, ArchiveLimits, TarZstdWriter, extract_tar_zstd};
use guardian_core::ArchivePath;
use std::io::Cursor;

#[test]
fn extraction_writes_only_to_a_new_destination() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("restore");
    let archive = archive()?;
    let inspection = extract_tar_zstd(
        archive.as_slice(),
        &destination,
        ArchiveLimits::conservative(),
    )?;
    assert_eq!(inspection.regular_files, 1);
    assert_eq!(
        std::fs::read(destination.join("srv/app/config.yaml"))?,
        b"mode: safe\n"
    );
    assert!(matches!(
        extract_tar_zstd(
            archive.as_slice(),
            &destination,
            ArchiveLimits::conservative()
        ),
        Err(ArchiveError::Invalid)
    ));
    Ok(())
}

#[test]
fn extraction_creates_missing_ancestors_for_a_multi_segment_root()
-> Result<(), Box<dyn std::error::Error>> {
    // Real tar never emits a separate entry for a path segment *above* the
    // captured root: capturing `/srv/app` yields only `srv/app` and its
    // descendants, never a lone `srv` entry first. This archive
    // deliberately matches that shape (no `srv` directory entry at all) —
    // unlike `archive()` below, which explicitly enumerates every level and
    // would have hidden this exact gap.
    let root = tempfile::tempdir()?;
    let destination = root.path().join("restore");
    let app = ArchivePath::parse("srv/app")?;
    let file = ArchivePath::parse("srv/app/config.yaml")?;
    let mut contents = Cursor::new(b"mode: safe\n".as_slice());
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_directory(&app)?;
    writer.append_file(
        &file,
        u64::try_from(contents.get_ref().len())?,
        &mut contents,
    )?;
    let archive = writer.finish()?;
    extract_tar_zstd(
        archive.as_slice(),
        &destination,
        ArchiveLimits::conservative(),
    )?;
    assert_eq!(
        std::fs::read(destination.join("srv/app/config.yaml"))?,
        b"mode: safe\n"
    );
    Ok(())
}

#[test]
fn failed_extraction_removes_its_new_destination() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("restore");
    assert!(matches!(
        extract_tar_zstd(
            b"not a zstd stream".as_slice(),
            &destination,
            ArchiveLimits::conservative()
        ),
        Err(ArchiveError::Invalid)
    ));
    assert!(!destination.exists());
    Ok(())
}

fn archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let directory = ArchivePath::parse("srv")?;
    let app = ArchivePath::parse("srv/app")?;
    let file = ArchivePath::parse("srv/app/config.yaml")?;
    let mut contents = Cursor::new(b"mode: safe\n".as_slice());
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_directory(&directory)?;
    writer.append_directory(&app)?;
    writer.append_file(
        &file,
        u64::try_from(contents.get_ref().len())?,
        &mut contents,
    )?;
    Ok(writer.finish()?)
}
