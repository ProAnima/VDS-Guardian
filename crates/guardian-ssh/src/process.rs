use std::{
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

pub(super) fn wait_for_exit(
    mut child: Child,
    total_timeout: Duration,
) -> Result<ExitStatus, WaitError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= total_timeout => {
                child.kill().map_err(|_| WaitError::Failed)?;
                child.wait().map_err(|_| WaitError::Failed)?;
                return Err(WaitError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(WaitError::Failed),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WaitError {
    TimedOut,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{WaitError, wait_for_exit};
    use std::{process::Command, time::Duration};

    #[test]
    fn deadline_kills_a_process_that_does_not_exit() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = sleeper();
        let child = command.spawn()?;
        assert_eq!(
            wait_for_exit(child, Duration::from_millis(10)),
            Err(WaitError::TimedOut)
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
