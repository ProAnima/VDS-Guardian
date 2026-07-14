use guardian_archive::{ArchiveLimits, TarZstdWriter, inspect_tar_zstd};
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

fn write_archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let directory = ArchivePath::parse("srv/app")?;
    let file = ArchivePath::parse("srv/app/config.yaml")?;
    let mut payload = Cursor::new(b"mode: safe\n".as_slice());
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_directory(&directory)?;
    writer.append_file(&file, u64::try_from(payload.get_ref().len())?, &mut payload)?;
    Ok(writer.finish()?)
}
