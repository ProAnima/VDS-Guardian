use std::{
    fs::File,
    io::{Read, Write},
    process::{Child, ChildStdout, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct CapturePump {
    activity: Receiver<()>,
    failed: Arc<AtomicBool>,
    join: JoinHandle<Result<(), ()>>,
}

impl CapturePump {
    pub(super) fn start(stdout: ChildStdout, output: File) -> Self {
        Self::start_inner(stdout, output, None)
    }

    pub(super) fn start_limited(stdout: ChildStdout, output: File, maximum: u64) -> Self {
        Self::start_inner(stdout, output, Some(maximum))
    }

    fn start_inner(stdout: ChildStdout, output: File, maximum: Option<u64>) -> Self {
        let (sender, activity) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let copy_failed = Arc::clone(&failed);
        let join = thread::spawn(move || copy_stream(stdout, output, sender, maximum, copy_failed));
        Self {
            activity,
            failed,
            join,
        }
    }

    pub(super) fn activity(&self) -> &Receiver<()> {
        &self.activity
    }

    pub(super) fn failed(&self) -> &AtomicBool {
        &self.failed
    }

    pub(super) fn finish(self) -> Result<(), ()> {
        self.join.join().map_err(|_| ())?
    }
}

pub(super) fn wait_for_stream(
    mut child: Child,
    total_timeout: Duration,
    idle_timeout: Duration,
    activity: &Receiver<()>,
    failed: &AtomicBool,
) -> Result<ExitStatus, StreamWaitError> {
    let started = Instant::now();
    let mut last_activity = started;
    loop {
        while activity.try_recv().is_ok() {
            last_activity = Instant::now();
        }
        if failed.load(Ordering::Relaxed) {
            return stop(child, StreamWaitError::Failed);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= total_timeout => {
                return stop(child, StreamWaitError::TimedOut);
            }
            Ok(None) if last_activity.elapsed() >= idle_timeout => {
                return stop(child, StreamWaitError::IdleTimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(StreamWaitError::Failed),
        }
    }
}

fn copy_stream(
    mut input: ChildStdout,
    mut output: File,
    activity: SyncSender<()>,
    maximum: Option<u64>,
    failed: Arc<AtomicBool>,
) -> Result<(), ()> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = match input.read(&mut buffer) {
            Ok(read) => read,
            Err(_) => return copy_failed(&failed),
        };
        if read == 0 {
            return output.sync_all().map_err(|_| ());
        }
        written = match written.checked_add(u64::try_from(read).map_err(|_| ())?) {
            Some(written) => written,
            None => return copy_failed(&failed),
        };
        if maximum.is_some_and(|limit| written > limit)
            || output.write_all(&buffer[..read]).is_err()
        {
            return copy_failed(&failed);
        }
        let _ = activity.try_send(());
    }
}

fn copy_failed(failed: &AtomicBool) -> Result<(), ()> {
    failed.store(true, Ordering::Relaxed);
    Err(())
}

fn stop(mut child: Child, error: StreamWaitError) -> Result<ExitStatus, StreamWaitError> {
    child.kill().map_err(|_| StreamWaitError::Failed)?;
    child.wait().map_err(|_| StreamWaitError::Failed)?;
    Err(error)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StreamWaitError {
    TimedOut,
    IdleTimedOut,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{CapturePump, StreamWaitError, wait_for_stream};
    use std::{
        process::Command,
        sync::{Arc, atomic::AtomicBool, mpsc},
        time::Duration,
    };

    #[test]
    fn idle_deadline_kills_a_silent_process() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = sleeper();
        let child = command.spawn()?;
        let (_, activity) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        assert_eq!(
            wait_for_stream(
                child,
                Duration::from_secs(1),
                Duration::from_millis(10),
                &activity,
                &failed,
            ),
            Err(StreamWaitError::IdleTimedOut)
        );
        Ok(())
    }

    #[test]
    fn output_limit_stops_a_stream_before_it_can_fill_disk()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = output_then_sleep();
        command.stdout(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("missing child stdout"))?;
        let output = tempfile::NamedTempFile::new()?.reopen()?;
        let pump = CapturePump::start_limited(stdout, output, 1);
        assert_eq!(
            wait_for_stream(
                child,
                Duration::from_secs(1),
                Duration::from_secs(1),
                pump.activity(),
                pump.failed(),
            ),
            Err(StreamWaitError::Failed)
        );
        assert!(pump.finish().is_err());
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

    #[cfg(windows)]
    fn output_then_sleep() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::Out.Write('AB'); Start-Sleep -Seconds 5",
        ]);
        command
    }

    #[cfg(not(windows))]
    fn output_then_sleep() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "printf AB; sleep 5"]);
        command
    }
}
