//! Tracks which SSH-backed jobs (capture, deploy) are currently running so a
//! separate cancel-job call can reach them. The RAII guard mirrors
//! `guardian-local-repository`'s `ProcessLock` deliberately — registration
//! must be released regardless of how the job ends, and a manual trailing
//! `unregister()` call could be bypassed by a future early return in a way
//! `Drop` cannot.

use crate::{CancellationHandle, RunId};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct JobRegistry(Mutex<HashMap<RunId, CancellationHandle>>);

impl JobRegistry {
    #[must_use]
    pub fn register(&self, run_id: RunId, handle: CancellationHandle) -> JobRegistration<'_> {
        if let Ok(mut jobs) = self.0.lock() {
            jobs.insert(run_id.clone(), handle);
        }
        JobRegistration {
            registry: self,
            run_id,
        }
    }

    /// Returns whether a job with this id was found and signalled.
    pub fn cancel(&self, run_id: &RunId) -> bool {
        match self.0.lock() {
            Ok(jobs) => jobs.get(run_id).is_some_and(|handle| {
                handle.cancel();
                true
            }),
            Err(_) => false,
        }
    }

    fn unregister(&self, run_id: &RunId) {
        if let Ok(mut jobs) = self.0.lock() {
            jobs.remove(run_id);
        }
    }
}

pub struct JobRegistration<'a> {
    registry: &'a JobRegistry,
    run_id: RunId,
}

impl Drop for JobRegistration<'_> {
    fn drop(&mut self) {
        self.registry.unregister(&self.run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::JobRegistry;
    use crate::{CancellationHandle, RunId};

    #[test]
    fn cancelling_a_registered_job_signals_its_handle() -> Result<(), Box<dyn std::error::Error>> {
        let registry = JobRegistry::default();
        let run_id = RunId::parse("run-001")?;
        let handle = CancellationHandle::new();
        let _registration = registry.register(run_id.clone(), handle.clone());
        assert!(registry.cancel(&run_id));
        assert!(handle.is_cancelled());
        Ok(())
    }

    #[test]
    fn cancelling_an_unknown_run_id_reports_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let registry = JobRegistry::default();
        assert!(!registry.cancel(&RunId::parse("run-missing")?));
        Ok(())
    }

    #[test]
    fn dropping_the_registration_unregisters_the_job() -> Result<(), Box<dyn std::error::Error>> {
        let registry = JobRegistry::default();
        let run_id = RunId::parse("run-002")?;
        let handle = CancellationHandle::new();
        {
            let _registration = registry.register(run_id.clone(), handle.clone());
        }
        assert!(!registry.cancel(&run_id));
        assert!(!handle.is_cancelled());
        Ok(())
    }
}
