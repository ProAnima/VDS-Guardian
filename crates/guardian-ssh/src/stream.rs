use guardian_core::CancellationHandle;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, ExitStatus},
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

/// A local payload reader handed to the push pump. Boxed rather than a
/// concrete `File` because the real source (a decrypted-payload reader from
/// `guardian-local-repository`) may need to keep a scratch-file guard alive
/// alongside the readable handle — something a bare `File` can't represent.
pub(super) type PushSource = Box<dyn Read + Send>;

/// The push-direction mirror of `CapturePump`: reads a local source and
/// writes into a child's stdin instead of the other way around. Unlike the
/// pull side (which doesn't know the remote's exact output size in advance),
/// a push already knows the payload's exact verified length, so it fails
/// closed on any mismatch — too many bytes mid-stream *or* too few at EOF —
/// rather than enforcing only a ceiling.
pub(super) struct PushPump {
    activity: Receiver<()>,
    failed: Arc<AtomicBool>,
    join: JoinHandle<Result<(), PushCopyError>>,
}

impl PushPump {
    pub(super) fn start(source: PushSource, stdin: ChildStdin, expected_bytes: u64) -> Self {
        let (sender, activity) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let copy_failed = Arc::clone(&failed);
        let join = thread::spawn(move || {
            copy_stream_to_child(source, stdin, expected_bytes, sender, copy_failed)
        });
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

    pub(super) fn finish(self) -> Result<(), PushCopyError> {
        self.join.join().map_err(|_| PushCopyError::Io)?
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PushCopyError {
    Io,
    ByteCountMismatch,
}

pub(super) fn wait_for_stream(
    mut child: Child,
    total_timeout: Duration,
    idle_timeout: Duration,
    activity: &Receiver<()>,
    failed: &AtomicBool,
    cancelled: &CancellationHandle,
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
        if cancelled.is_cancelled() {
            return stop(child, StreamWaitError::Cancelled);
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

/// Reads `source` to completion, writing every byte into `stdin`. Always
/// explicitly drops `stdin` before returning on every path (success,
/// mismatch, or I/O error) — that close is what signals EOF to whatever
/// remote command is reading the other end of the pipe, letting it exit
/// (successfully or not) instead of blocking forever on more input.
fn copy_stream_to_child(
    mut source: PushSource,
    mut stdin: ChildStdin,
    expected_bytes: u64,
    activity: SyncSender<()>,
    failed: Arc<AtomicBool>,
) -> Result<(), PushCopyError> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = match source.read(&mut buffer) {
            Ok(read) => read,
            Err(_) => {
                drop(stdin);
                return push_copy_failed(&failed, PushCopyError::Io);
            }
        };
        if read == 0 {
            drop(stdin);
            return if written == expected_bytes {
                Ok(())
            } else {
                push_copy_failed(&failed, PushCopyError::ByteCountMismatch)
            };
        }
        let read_bytes = match u64::try_from(read) {
            Ok(read_bytes) => read_bytes,
            Err(_) => {
                drop(stdin);
                return push_copy_failed(&failed, PushCopyError::Io);
            }
        };
        written = match written.checked_add(read_bytes) {
            Some(written) => written,
            None => {
                drop(stdin);
                return push_copy_failed(&failed, PushCopyError::Io);
            }
        };
        if written > expected_bytes {
            drop(stdin);
            return push_copy_failed(&failed, PushCopyError::ByteCountMismatch);
        }
        if stdin.write_all(&buffer[..read]).is_err() {
            drop(stdin);
            return push_copy_failed(&failed, PushCopyError::Io);
        }
        let _ = activity.try_send(());
    }
}

fn copy_failed(failed: &AtomicBool) -> Result<(), ()> {
    failed.store(true, Ordering::Relaxed);
    Err(())
}

fn push_copy_failed(failed: &AtomicBool, error: PushCopyError) -> Result<(), PushCopyError> {
    failed.store(true, Ordering::Relaxed);
    Err(error)
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
    Cancelled,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{CapturePump, PushCopyError, PushPump, StreamWaitError, wait_for_stream};
    use guardian_core::CancellationHandle;
    use std::{
        fs::File,
        io::Write,
        process::Command,
        sync::{Arc, atomic::AtomicBool, mpsc},
        thread,
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
                &CancellationHandle::new(),
            ),
            Err(StreamWaitError::IdleTimedOut)
        );
        Ok(())
    }

    #[test]
    fn cancelling_from_another_thread_kills_a_still_running_stream()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = sleeper();
        let child = command.spawn()?;
        let (_, activity) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let handle = CancellationHandle::new();
        let cancel_handle = handle.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel_handle.cancel();
        });
        assert_eq!(
            wait_for_stream(
                child,
                Duration::from_secs(5),
                Duration::from_secs(5),
                &activity,
                &failed,
                &handle,
            ),
            Err(StreamWaitError::Cancelled)
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
                &CancellationHandle::new(),
            ),
            Err(StreamWaitError::Failed)
        );
        assert!(pump.finish().is_err());
        Ok(())
    }

    #[test]
    fn push_pump_round_trips_and_signals_eof_by_closing_stdin()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = drain_stdin();
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("missing child stdin"))?;
        let payload = b"push-pump-payload";
        let source = source_file(payload)?;
        let pump = PushPump::start(Box::new(source), stdin, payload.len() as u64);
        let status = wait_for_stream(
            child,
            Duration::from_secs(2),
            Duration::from_secs(2),
            pump.activity(),
            pump.failed(),
            &CancellationHandle::new(),
        );
        assert!(matches!(status, Ok(status) if status.success()));
        assert_eq!(pump.finish(), Ok(()));
        Ok(())
    }

    #[test]
    fn push_pump_fails_closed_when_the_source_is_shorter_than_expected()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = drain_stdin();
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("missing child stdin"))?;
        let source = source_file(b"short")?;
        let pump = PushPump::start(Box::new(source), stdin, 1_000);
        let _ = wait_for_stream(
            child,
            Duration::from_secs(2),
            Duration::from_secs(2),
            pump.activity(),
            pump.failed(),
            &CancellationHandle::new(),
        );
        assert_eq!(pump.finish(), Err(PushCopyError::ByteCountMismatch));
        Ok(())
    }

    #[test]
    fn push_pump_fails_closed_when_the_source_is_longer_than_expected()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = drain_stdin();
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("missing child stdin"))?;
        let source = source_file(b"this source is definitely longer than expected")?;
        let pump = PushPump::start(Box::new(source), stdin, 4);
        let _ = wait_for_stream(
            child,
            Duration::from_secs(2),
            Duration::from_secs(2),
            pump.activity(),
            pump.failed(),
            &CancellationHandle::new(),
        );
        assert_eq!(pump.finish(), Err(PushCopyError::ByteCountMismatch));
        Ok(())
    }

    /// Load-bearing on both platforms: a remote that stalls mid-push must be
    /// killed by the idle deadline without hanging the pump thread. Unix
    /// `write()`-into-a-closed-pipe behavior on `child.kill()` is well
    /// understood; Windows `TerminateProcess`-vs-pending-`WriteFile`
    /// interaction is exactly the kind of assumption worth an explicit test
    /// rather than trusting either way.
    #[test]
    fn killing_a_stalled_remote_mid_push_does_not_hang_the_pump()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut command = read_a_little_then_sleep();
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("missing child stdin"))?;
        // Comfortably larger than any plausible OS pipe buffer, so the pump
        // thread is guaranteed to block on `write_all` once the child stops
        // reading past its first byte.
        let payload = vec![0_u8; 1024 * 1024];
        let source = source_file(&payload)?;
        let pump = PushPump::start(Box::new(source), stdin, payload.len() as u64);
        let result = wait_for_stream(
            child,
            Duration::from_secs(5),
            Duration::from_millis(200),
            pump.activity(),
            pump.failed(),
            &CancellationHandle::new(),
        );
        assert_eq!(result, Err(StreamWaitError::IdleTimedOut));
        assert!(pump.finish().is_err());
        Ok(())
    }

    fn source_file(bytes: &[u8]) -> Result<File, Box<dyn std::error::Error>> {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(bytes)?;
        Ok(file.reopen()?)
    }

    #[cfg(windows)]
    fn drain_stdin() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::In.ReadToEnd() | Out-Null",
        ]);
        command
    }

    #[cfg(not(windows))]
    fn drain_stdin() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "cat >/dev/null"]);
        command
    }

    #[cfg(windows)]
    fn read_a_little_then_sleep() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$stream = [Console]::OpenStandardInput(); $buf = New-Object byte[] 1; $stream.Read($buf, 0, 1) | Out-Null; Start-Sleep -Seconds 5",
        ]);
        command
    }

    #[cfg(not(windows))]
    fn read_a_little_then_sleep() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "dd bs=1 count=1 >/dev/null 2>&1; sleep 5"]);
        command
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
