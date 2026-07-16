//! Pushes a sealed backup's decrypted payloads onto a remote target during
//! deploy — see `docs/adr/0007-remote-deploy-to-a-new-vds.md`.

use crate::stream;
use crate::{PinnedHost, SshError, SshUser, SystemOpenSsh, map_wait_error, process, shell_quote};
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushResult {}

impl SystemOpenSsh {
    /// Pushes a decrypted, still-compressed tar.zst stream onto a remote
    /// target directory that must not already exist. The remote command
    /// extracts into a freshly created, uniquely named sibling temp
    /// directory and atomically renames it into place only on full success —
    /// see `docs/adr/0007-remote-deploy-to-a-new-vds.md`.
    pub fn push_filesystem_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        target_path: &str,
        source: impl Read + Send + 'static,
        expected_bytes: u64,
    ) -> Result<PushResult, SshError> {
        self.push_to(
            host,
            user,
            identity_file,
            push_filesystem_command(target_path),
            Box::new(source),
            expected_bytes,
        )
    }

    /// Pushes a decrypted, still-compressed raw zstd stream onto
    /// `<target_path>/database.sqlite`, which must not already exist.
    pub fn push_database_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        target_path: &str,
        source: impl Read + Send + 'static,
        expected_bytes: u64,
    ) -> Result<PushResult, SshError> {
        self.push_to(
            host,
            user,
            identity_file,
            push_database_command(target_path),
            Box::new(source),
            expected_bytes,
        )
    }

    /// Read-only preflight: reports whether `target_path` is currently
    /// absent on the remote host, without pushing anything. Used at plan
    /// time to give the operator early feedback before they type the
    /// confirmation phrase; the actual push commands re-check absence
    /// themselves regardless, so this is a convenience, not the enforcement.
    pub fn probe_target_absent(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        target_path: &str,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.target_absence_probe_arguments(
                host,
                user,
                identity_file,
                known_hosts.path(),
                target_path,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        Ok(status.success())
    }

    #[must_use]
    pub fn target_absence_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        target_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            target_absence_probe_command(target_path).into(),
        )
    }

    #[must_use]
    pub fn push_filesystem_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        target_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            push_filesystem_command(target_path).into(),
        )
    }

    #[must_use]
    pub fn push_database_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        target_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            push_database_command(target_path).into(),
        )
    }

    fn push_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        remote_command: String,
        source: stream::PushSource,
        expected_bytes: u64,
    ) -> Result<PushResult, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let mut child = match self
            .new_command()
            .args(self.arguments_for_command(
                host,
                user,
                identity_file,
                known_hosts.path(),
                remote_command.into(),
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return Err(SshError::LaunchFailed),
        };
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => return Err(SshError::LocalIo),
        };
        let pump = stream::PushPump::start(source, stdin, expected_bytes);
        let status = match stream::wait_for_stream(
            child,
            self.total_timeout,
            self.idle_timeout,
            pump.activity(),
            pump.failed(),
            &self.cancellation,
        ) {
            Ok(status) => status,
            Err(stream::StreamWaitError::TimedOut) => {
                let _ = pump.finish();
                return Err(SshError::TimedOut);
            }
            Err(stream::StreamWaitError::IdleTimedOut) => {
                let _ = pump.finish();
                return Err(SshError::IdleTimedOut);
            }
            Err(stream::StreamWaitError::Cancelled) => {
                let _ = pump.finish();
                return Err(SshError::Cancelled);
            }
            Err(stream::StreamWaitError::Failed) => {
                return Err(push_finish_error(pump.finish()));
            }
        };
        if let Err(error) = pump.finish() {
            return Err(push_finish_error(Err(error)));
        }
        if !status.success() {
            return Err(SshError::CaptureFailed);
        }
        Ok(PushResult {})
    }
}

fn push_finish_error(result: Result<(), stream::PushCopyError>) -> SshError {
    match result {
        Ok(()) | Err(stream::PushCopyError::Io) => SshError::LocalIo,
        Err(stream::PushCopyError::ByteCountMismatch) => SshError::ByteCountMismatch,
    }
}

fn target_absence_probe_command(target_path: &str) -> String {
    format!("[ ! -e {} ]", shell_quote(target_path))
}

/// Extracts a tar.zst stream (read from stdin) into `<target_path>`, which
/// must not already exist. Extracts into a freshly created, uniquely named
/// sibling temp directory (never a fixed, guessable name — a predictable
/// sibling would let an unconditional cleanup step destroy something
/// unrelated that happened to already exist there) and atomically renames it
/// into place only on full success. `--no-same-owner --no-same-permissions`
/// mean every entry *inside* the extracted tree lands owned by the SSH
/// session's own account with ordinary umask-based permissions, never the
/// archive-recorded owner or mode bits (including setuid/setgid) — see
/// `docs/adr/0007-remote-deploy-to-a-new-vds.md` and `docs/SECURITY_
/// MODEL.md`. The root of that tree is a different case: `mktemp -d` always
/// creates it `0700` regardless of umask (that restriction is the whole
/// point of `mktemp` — a predictable, umask-derived mode on a temp path
/// would defeat it), and `mv -n` renames that entry as-is, so without an
/// explicit `chmod` the *renamed* target itself — not its contents — would
/// stay owner-only and lock out whatever account is actually meant to use
/// the deployed tree. `chmod 755` restores an ordinary, predictable mode
/// before anything is extracted into it.
fn push_filesystem_command(target_path: &str) -> String {
    let target = shell_quote(target_path);
    format!(
        "target={target}; parent=$(dirname -- \"$target\"); [ ! -e \"$target\" ] || exit 1; mkdir -p -- \"$parent\" || exit 1; tmp=$(mktemp -d -- \"$parent/.guardian-deploy-tmp.XXXXXX\") || exit 1; chmod 755 -- \"$tmp\" || exit 1; tar --extract --file=- --zstd --no-same-owner --no-same-permissions --one-file-system -C \"$tmp\" --; status=$?; if [ \"$status\" -eq 0 ]; then mv -n -- \"$tmp\" \"$target\"; [ ! -e \"$tmp\" ] || status=1; fi; [ \"$status\" -eq 0 ] || rm -rf -- \"$tmp\"; exit \"$status\""
    )
}

/// Decompresses a raw zstd stream (read from stdin) to
/// `<target_path>/database.sqlite`, which must not already exist. Guards
/// that file specifically, not `target_path` itself, since a preceding
/// filesystem push may have already legitimately created `target_path` —
/// which is therefore also already the guaranteed-existing parent this
/// function's own temp file is created inside. Uses a freshly created,
/// uniquely named sibling temp file for the same reason `push_filesystem_
/// command` does, and the same reason it needs an explicit `chmod`: bare
/// `mktemp` always creates its file `0600`, and `mv -n` renames it as-is,
/// so `chmod 644` restores an ordinary, predictable mode before the renamed
/// file replaces the umask-based mode a plain shell redirect used to leave
/// it with.
fn push_database_command(target_path: &str) -> String {
    let target = shell_quote(&format!("{target_path}/database.sqlite"));
    format!(
        "target={target}; parent=$(dirname -- \"$target\"); [ ! -e \"$target\" ] || exit 1; tmp=$(mktemp -- \"$parent/.guardian-deploy-tmp.XXXXXX\") || exit 1; chmod 644 -- \"$tmp\" || exit 1; zstd -q -d -c > \"$tmp\"; status=$?; if [ \"$status\" -eq 0 ]; then mv -n -- \"$tmp\" \"$target\"; [ ! -e \"$tmp\" ] || status=1; fi; [ \"$status\" -eq 0 ] || rm -f -- \"$tmp\"; exit \"$status\""
    )
}
