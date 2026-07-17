use crate::ArchiveError;
use guardian_core::CancellationHandle;
use std::io::{self, Read};

pub(crate) struct CancellationReader<R> {
    inner: R,
    cancellation: Option<CancellationHandle>,
}

impl<R> CancellationReader<R> {
    pub(crate) fn new(inner: R, cancellation: Option<&CancellationHandle>) -> Self {
        Self {
            inner,
            cancellation: cancellation.cloned(),
        }
    }
}

impl<R: Read> Read for CancellationReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if is_cancelled(self.cancellation.as_ref()) {
            return Err(io::Error::other("operation cancelled"));
        }
        self.inner.read(buffer)
    }
}

pub(crate) fn check(cancellation: Option<&CancellationHandle>) -> Result<(), ArchiveError> {
    if is_cancelled(cancellation) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn map_read(
    _error: io::Error,
    cancellation: Option<&CancellationHandle>,
) -> ArchiveError {
    if is_cancelled(cancellation) {
        ArchiveError::Cancelled
    } else {
        ArchiveError::Invalid
    }
}

fn is_cancelled(cancellation: Option<&CancellationHandle>) -> bool {
    cancellation.is_some_and(CancellationHandle::is_cancelled)
}
