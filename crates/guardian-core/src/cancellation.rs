//! A minimal cross-thread cancellation signal for long-running SSH-backed
//! operations (capture, deploy). Deliberately hand-rolled rather than an
//! async runtime's cancellation token: the adapters that consume this
//! (`guardian-ssh`) are architecturally synchronous poll loops, not async
//! tasks, and already use the identical `Arc<AtomicBool>` shape internally
//! (`guardian-ssh`'s stream pump "failed" flag) for the same kind of
//! cross-thread signal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct CancellationHandle(Arc<AtomicBool>);

impl CancellationHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::CancellationHandle;

    #[test]
    fn a_fresh_handle_is_not_cancelled() {
        assert!(!CancellationHandle::new().is_cancelled());
    }

    #[test]
    fn cancelling_is_visible_through_a_clone() {
        let handle = CancellationHandle::new();
        let clone = handle.clone();
        clone.cancel();
        assert!(handle.is_cancelled());
    }
}
