use guardian_core::CancellationHandle;
use std::{
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

pub(super) fn wait_for_exit(
    mut child: Child,
    total_timeout: Duration,
    cancelled: &CancellationHandle,
) -> Result<ExitStatus, WaitError> {
    let started = Instant::now();
    loop {
        if cancelled.is_cancelled() {
            return stop(child, WaitError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= total_timeout => {
                return stop(child, WaitError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(WaitError::Failed),
        }
    }
}

fn stop(mut child: Child, error: WaitError) -> Result<ExitStatus, WaitError> {
    child.kill().map_err(|_| WaitError::Failed)?;
    child.wait().map_err(|_| WaitError::Failed)?;
    Err(error)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WaitError {
    TimedOut,
    Cancelled,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{WaitError, wait_for_exit};
    use guardian_core::CancellationHandle;
    use std::{process::Command, thread, time::Duration};

    #[test]
    fn deadline_kills_a_process_that_does_not_exit() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = sleeper();
        let child = command.spawn()?;
        assert_eq!(
            wait_for_exit(child, Duration::from_millis(10), &CancellationHandle::new()),
            Err(WaitError::TimedOut)
        );
        Ok(())
    }

    #[test]
    fn cancelling_from_another_thread_kills_a_still_running_process()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = sleeper();
        let child = command.spawn()?;
        let handle = CancellationHandle::new();
        let cancel_handle = handle.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel_handle.cancel();
        });
        assert_eq!(
            wait_for_exit(child, Duration::from_secs(5), &handle),
            Err(WaitError::Cancelled)
        );
        Ok(())
    }

    #[cfg(windows)]
    fn sleeper() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 5",
        ]);
        command
    }

    #[cfg(not(windows))]
    fn sleeper() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        command
    }
}
