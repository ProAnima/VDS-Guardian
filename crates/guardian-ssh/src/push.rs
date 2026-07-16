//! Pushes a sealed backup's decrypted payloads onto a remote target during
//! deploy — see `docs/adr/0007-remote-deploy-to-a-new-vds.md`.

use crate::stream;
use crate::{PinnedHost, SshError, SshUser, SystemOpenSsh, map_wait_error, process, shell_quote};
use guardian_core::RunId;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushResult {}

/// A remote deploy root plus the deploy attempt's own `RunId` — together
/// they identify one combined deploy's shared staging directory. Bundled
/// into one type specifically to keep `push_filesystem_into_staging_to`/
/// `push_database_into_staging_to`/`finalize_deploy_to` under this
/// workspace's argument-count budget; `target_path` and `run_id` are always
/// used together to compute the same staging name regardless.
#[derive(Debug, Clone, Copy)]
pub struct StagingTarget<'a> {
    pub target_path: &'a str,
    pub run_id: &'a RunId,
}

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

    /// Pushes a decrypted, still-compressed tar.zst stream into a shared
    /// staging directory sibling to `target_path` — never renamed into
    /// place by this call. Used only when a database payload also exists:
    /// a combined deploy stages both payloads first and finalizes with one
    /// separate `finalize_deploy_to` call, so a failed second payload never
    /// leaves the first payload's content live at `target_path`. When there
    /// is no database payload, `push_filesystem_to` (single push, immediate
    /// rename) is already fully atomic and is used instead — see
    /// `docs/adr/0007-remote-deploy-to-a-new-vds.md`.
    pub fn push_filesystem_into_staging_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        staging: StagingTarget<'_>,
        source: impl Read + Send + 'static,
        expected_bytes: u64,
    ) -> Result<PushResult, SshError> {
        self.push_to(
            host,
            user,
            identity_file,
            push_filesystem_into_staging_command(staging),
            Box::new(source),
            expected_bytes,
        )
    }

    /// Pushes a decrypted, still-compressed raw zstd stream into
    /// `<staging>/database.sqlite`, where `<staging>` must already exist —
    /// created by a preceding `push_filesystem_into_staging_to` call in the
    /// same deploy attempt. Never renamed into place by this call; see
    /// `push_filesystem_into_staging_to`'s doc comment for why.
    pub fn push_database_into_staging_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        staging: StagingTarget<'_>,
        source: impl Read + Send + 'static,
        expected_bytes: u64,
    ) -> Result<PushResult, SshError> {
        self.push_to(
            host,
            user,
            identity_file,
            push_database_into_staging_command(staging),
            Box::new(source),
            expected_bytes,
        )
    }

    /// Publishes a combined deploy's staged payloads with the single final
    /// rename that makes both visible at `target_path` atomically. Re-checks
    /// `target_path` is still absent immediately before the rename, and
    /// cleans up the staging directory on any failure (including a
    /// mid-rename race where something else claimed `target_path` first).
    pub fn finalize_deploy_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        staging: StagingTarget<'_>,
    ) -> Result<(), SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.finalize_deploy_arguments(
                host,
                user,
                identity_file,
                known_hosts.as_ref(),
                staging,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        status
            .success()
            .then_some(())
            .ok_or(SshError::CaptureFailed)
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
                known_hosts.as_ref(),
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
    pub fn push_filesystem_into_staging_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        staging: StagingTarget<'_>,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            push_filesystem_into_staging_command(staging).into(),
        )
    }

    #[must_use]
    pub fn push_database_into_staging_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        staging: StagingTarget<'_>,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            push_database_into_staging_command(staging).into(),
        )
    }

    #[must_use]
    pub fn finalize_deploy_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        staging: StagingTarget<'_>,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            finalize_deploy_command(staging).into(),
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
                known_hosts.as_ref(),
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

/// The shared staging-directory name a combined deploy's three separate SSH
/// invocations (filesystem-into-staging, database-into-staging, finalize)
/// all agree on, without any of them reading a prior invocation's output —
/// `run_id` is fresh per deploy attempt and validated (`guardian-core::
/// RunId`) to ASCII alphanumeric plus `-`/`_` only, so it can be embedded
/// directly with no `shell_quote` escaping, unlike `target_path` (arbitrary
/// POSIX path text, always quoted). Centralized here so the three templates
/// below can never drift onto different naming schemes. Trusts that callers
/// mint high-entropy run ids (both current callers do: CLI's `OsRng`-backed
/// `random_run_id()`, desktop's `crypto.randomUUID()`) — `RunId::parse`
/// itself only validates charset and length, not entropy.
fn deploy_staging_assignment(run_id: &RunId) -> String {
    format!(
        "staging=\"$parent/.guardian-deploy-staging.{}\"",
        run_id.as_str()
    )
}

/// Extracts a tar.zst stream (read from stdin) into a shared staging
/// directory sibling to `target_path`, without renaming it into place —
/// see `SystemOpenSsh::push_filesystem_into_staging_to`'s doc comment. Fails
/// closed if either `target_path` or the staging directory already exist,
/// and cleans up the staging directory entirely on its own failure (a
/// failed first stage abandons the whole combined-deploy attempt).
fn push_filesystem_into_staging_command(staging: StagingTarget<'_>) -> String {
    let target = shell_quote(staging.target_path);
    let staging_assignment = deploy_staging_assignment(staging.run_id);
    format!(
        "target={target}; parent=$(dirname -- \"$target\"); {staging_assignment}; [ ! -e \"$target\" ] || exit 1; [ ! -e \"$staging\" ] || exit 1; mkdir -p -- \"$parent\" || exit 1; mkdir -- \"$staging\" || exit 1; chmod 755 -- \"$staging\" || exit 1; tar --extract --file=- --zstd --no-same-owner --no-same-permissions --one-file-system -C \"$staging\" --; status=$?; [ \"$status\" -eq 0 ] || rm -rf -- \"$staging\"; exit \"$status\""
    )
}

/// Decompresses a raw zstd stream (read from stdin) to
/// `<staging>/database.sqlite`, where `<staging>` must already exist —
/// created by a preceding `push_filesystem_into_staging_command` run in the
/// same deploy attempt (same `run_id`, so the same staging name). Never
/// renames into place. On its own failure, cleans up the *entire* staging
/// tree, not just the one file it was writing — a failed second stage
/// abandons the whole attempt, including the first stage's already-staged
/// content.
fn push_database_into_staging_command(staging: StagingTarget<'_>) -> String {
    let target = shell_quote(staging.target_path);
    let staging_assignment = deploy_staging_assignment(staging.run_id);
    format!(
        "target={target}; parent=$(dirname -- \"$target\"); {staging_assignment}; [ -d \"$staging\" ] || exit 1; [ ! -e \"$target\" ] || exit 1; zstd -q -d -c > \"$staging/database.sqlite\"; status=$?; [ \"$status\" -eq 0 ] || rm -rf -- \"$staging\"; exit \"$status\""
    )
}

/// Publishes a combined deploy's staged payloads with the one rename that
/// makes both visible at `target_path` atomically — see
/// `SystemOpenSsh::finalize_deploy_to`'s doc comment. Requires the staging
/// directory to exist and `target_path` to still be absent immediately
/// before the rename; cleans up the staging directory on any failure,
/// including a mid-rename race where something else claimed `target_path`
/// first.
fn finalize_deploy_command(staging: StagingTarget<'_>) -> String {
    let target = shell_quote(staging.target_path);
    let staging_assignment = deploy_staging_assignment(staging.run_id);
    format!(
        "target={target}; parent=$(dirname -- \"$target\"); {staging_assignment}; [ -e \"$staging\" ] || exit 1; [ ! -e \"$target\" ] || exit 1; mv -n -- \"$staging\" \"$target\"; status=$?; [ \"$status\" -ne 0 ] || [ ! -e \"$staging\" ] || status=1; [ \"$status\" -eq 0 ] || rm -rf -- \"$staging\"; exit \"$status\""
    )
}
