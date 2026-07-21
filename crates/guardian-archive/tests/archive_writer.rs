use guardian_archive::{
    ArchiveEntryKind, ArchiveLimits, TarZstdWriter, inspect_tar_zstd, list_tar_zstd_entries,
};
use guardian_core::ArchivePath;
use std::io::Cursor;

#[test]
fn writer_is_deterministic_and_emits_a_valid_archive() -> Result<(), Box<dyn std::error::Error>> {
    let first = write_archive()?;
    let second = write_archive()?;
    assert_eq!(first, second);
    let inspection = inspect_tar_zstd(first.as_slice(), ArchiveLimits::conservative())?;
    assert_eq!(inspection.entries, 2);
    assert_eq!(inspection.directories, 1);
    assert_eq!(inspection.regular_files, 1);
    Ok(())
}

#[test]
fn archive_entries_are_returned_as_a_bounded_page() -> Result<(), Box<dyn std::error::Error>> {
    let archive = write_archive()?;
    let first = list_tar_zstd_entries(archive.as_slice(), ArchiveLimits::conservative(), 0, 1)?;
    assert_eq!(first.total_entries, 2);
    assert_eq!(first.next_offset, Some(1));
    assert_eq!(first.entries[0].path, "srv/app");
    assert_eq!(first.entries[0].kind, ArchiveEntryKind::Directory);
    let second = list_tar_zstd_entries(archive.as_slice(), ArchiveLimits::conservative(), 1, 1)?;
    assert_eq!(second.entries[0].path, "srv/app/config.yaml");
    assert_eq!(second.entries[0].size, 11);
    Ok(())
}

fn write_archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let directory = ArchivePath::parse("srv/app")?;
    let file = ArchivePath::parse("srv/app/config.yaml")?;
    let mut payload = Cursor::new(b"mode: safe\n".as_slice());
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_directory(&directory)?;
    writer.append_file(&file, u64::try_from(payload.get_ref().len())?, &mut payload)?;
    Ok(writer.finish()?)
}
