use guardian_archive::{
    ArchiveError, ArchiveLimits, TarZstdWriter, extract_tar_zstd,
    extract_tar_zstd_with_cancellation,
};
use guardian_core::{ArchivePath, CancellationHandle};
use std::io::{self, Cursor, Read};

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

#[test]
fn cancelled_extraction_removes_its_partial_destination() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let destination = root.path().join("restore");
    let handle = CancellationHandle::new();
    let archive = large_archive()?;
    let source = CancelAfterReads::new(Cursor::new(archive), handle.clone(), 100);
    assert!(matches!(
        extract_tar_zstd_with_cancellation(
            source,
            &destination,
            ArchiveLimits::conservative(),
            &handle,
        ),
        Err(ArchiveError::Cancelled)
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

fn large_archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let file = ArchivePath::parse("srv/app/data.bin")?;
    let contents = (0..1_000_000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let mut source = Cursor::new(contents);
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_file(&file, u64::try_from(source.get_ref().len())?, &mut source)?;
    Ok(writer.finish()?)
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
