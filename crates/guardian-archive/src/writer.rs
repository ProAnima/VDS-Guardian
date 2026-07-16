use guardian_core::ArchivePath;
use std::io::{self, Cursor, Read, Write};
use tar::{Builder, EntryType, Header};
use thiserror::Error;

/// Produces deterministic tar.zst streams from already validated logical paths.
pub struct TarZstdWriter<W: Write> {
    builder: Builder<zstd::stream::write::Encoder<'static, W>>,
}

impl<W: Write> TarZstdWriter<W> {
    pub fn new(destination: W) -> Result<Self, ArchiveWriteError> {
        let encoder = zstd::stream::write::Encoder::new(destination, 0)
            .map_err(|_| ArchiveWriteError::Write)?;
        Ok(Self {
            builder: Builder::new(encoder),
        })
    }

    pub fn append_file(
        &mut self,
        path: &ArchivePath,
        size: u64,
        source: &mut impl Read,
    ) -> Result<(), ArchiveWriteError> {
        self.append(path, EntryType::Regular, 0o644, size, source)
    }

    pub fn append_directory(&mut self, path: &ArchivePath) -> Result<(), ArchiveWriteError> {
        self.append(path, EntryType::Directory, 0o755, 0, &mut Cursor::new([]))
    }

    pub fn finish(self) -> Result<W, ArchiveWriteError> {
        let encoder = self
            .builder
            .into_inner()
            .map_err(|_| ArchiveWriteError::Write)?;
        encoder.finish().map_err(|_| ArchiveWriteError::Write)
    }

    fn append(
        &mut self,
        path: &ArchivePath,
        entry_type: EntryType,
        mode: u32,
        size: u64,
        source: &mut impl Read,
    ) -> Result<(), ArchiveWriteError> {
        let mut header = Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_mode(mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(size);
        header.set_cksum();
        // Real tar writers always suffix a directory member's own name with
        // `/`; matching that here keeps this writer's output shaped like a
        // real tar stream, which is what the reader side actually has to
        // handle (see `parse_entry_path` in `lib.rs`).
        let name = if entry_type == EntryType::Directory {
            format!("{}/", path.as_str())
        } else {
            path.as_str().to_owned()
        };
        self.builder
            .append_data(&mut header, name, source)
            .map_err(|_| ArchiveWriteError::Write)
    }
}

#[derive(Debug, Error)]
pub enum ArchiveWriteError {
    #[error("unable to write backup archive")]
    Write,
}

impl From<io::Error> for ArchiveWriteError {
    fn from(_: io::Error) -> Self {
        Self::Write
    }
}
