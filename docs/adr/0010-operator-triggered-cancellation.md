# ADR 0010: Operator-triggered cancellation

## Status

Accepted.

## Context

The last item sitting at 0% in Milestone 2's exit gate: a stuck SSH-backed
operation could only be stopped by an automatic idle/total timeout — there
was no operator cancel command, job handle, or cancellation token anywhere
in the codebase.

`docs/ARCHITECTURE.md` claimed otherwise: "Jobs have cooperative
cancellation; process trees and remote commands receive bounded shutdown
before forced termination." This was false. What existed was
deadline-triggered forced termination (`guardian-ssh`'s `stream::
wait_for_stream`/`process::wait_for_exit` poll loops calling `Child::kill()`
once a timeout elapsed) — not "cooperative" in the sense of a job noticing
a request and unwinding on its own, and nothing operator-triggered at all.
`docs/SECURITY_MODEL.md` and `docs/SSH_CAPTURE.md` already described this
gap accurately; only `ARCHITECTURE.md` needed correcting, independent of
whatever shipped here.

`guardian_core::state::BackupState::Cancelled` existed as a modeled-but-
never-constructed enum variant. It stays that way after this change —
`BackupState` itself is disconnected from every real code path (the only
reference anywhere was its own `pub use` re-export), and the load-bearing
mechanism for recording *why* a job stopped is the separate, already-in-use
`write_capture_audit`/`write_deploy_audit` string-state parameter. Wiring up
`BackupState` for real is a larger, separate, pre-existing gap.

Scope: **CLI and desktop**, for SSH-backed operations only — capture
(desktop-only today) and deploy (both surfaces). Explicitly **not** local
restore extraction: no SSH child process is involved there, so it needs a
different mechanism (checking a signal inside `guardian-archive`'s copy
loops) and has a different risk profile (typically seconds to minutes, not
the multi-hour risk of a stalled network connection). Deferred, not
forgotten — see "Consequences" below.

## Decision

### `CancellationHandle` lives in `guardian-core`

A minimal, cheap-clone cross-thread signal (`Arc<AtomicBool>` under the
hood, `.cancel()`/`.is_cancelled()`), placed in `guardian-core` rather than
`guardian-ssh` and re-exported from there. This is the same shape
`guardian-ssh`'s own stream pump already uses internally for its `failed`
flag — not a new pattern. Placed in core specifically so a later
restore-extraction-cancellation slice can reuse it without
`guardian-archive` ever needing to depend on `guardian-ssh`. Deliberately
hand-rolled rather than an async runtime's cancellation token: the code
that consumes this is architecturally synchronous (`thread::sleep`-based
poll loops), and `tokio_util::sync::CancellationToken`'s async-await
surface has nothing to offer a loop that can't await.

### Two functions to change in `guardian-ssh`, zero changes to composition roots

`process::wait_for_exit` and `stream::wait_for_stream` gate every one of
`SystemOpenSsh`'s roughly 13 public methods. Both gained a `cancelled:
&CancellationHandle` parameter (a distinct type from `wait_for_stream`'s
existing `failed: &AtomicBool`, deliberately — two same-typed adjacent
parameters invite a transposition bug the compiler can't catch; two
different types can't be silently swapped), checked each poll tick in the
same position the existing `failed`/timeout checks already run. Both
`WaitError` and `StreamWaitError` gained a real `Cancelled` variant rather
than reusing `Failed` — conflating "the wait itself errored" with
"deliberately killed a healthy child because the operator asked" would
have made the difference invisible to both callers and tests, at zero
savings since both enums are private to this crate. `SystemOpenSsh` gained
a `cancellation: CancellationHandle` field and a `with_cancellation`
builder matching its existing `with_connect_timeout`/`with_total_timeout`/
`with_idle_timeout` shape exactly.

`guardian-capture` and `guardian-deploy` needed **zero changes** — they
already take `ssh: &'a SystemOpenSsh` as a given reference; only the code
that *constructs* that instance (desktop's `job_commands.rs`/
`deploy_commands.rs`, the CLI's `deploy.rs`) needed to call
`.with_cancellation(...)` before handing it in.

### Errors are decided at the composition-root caller, not inside shared enums

`guardian-core`'s `CapturePortError`/`CaptureUseCaseError` and
`DeployError` already collapse *every* `SshError` variant into one generic
bucket (`CapturePortError::Transport`, `DeployError::PushFailed`) for
reasons unrelated to this change — confirmed by reading the actual
adapters, not assumed. Widening these shared enums with a `Cancelled`
variant would have meant touching already-reviewed domain code for a
distinction cancellation would only immediately re-collapse anyway.
Instead, each caller that owns the `CancellationHandle` (`job_commands.rs`,
`deploy_commands.rs`, `guardian-cli`'s `deploy.rs`) checks
`handle.is_cancelled()` itself at the point the composition call fails,
and uses that single check to decide between a distinct `*Failure::
cancelled()` response and the existing generic failure, and between
`"cancelled"` and `"failed"` as the audit-state string written to the
already-existing `write_capture_audit`/`write_deploy_audit` calls (no
schema change — both already accept an arbitrary `&'static str` state).

This distinction is required, not polish: `CODEX.md`'s non-negotiable
invariants name "cancelled" as a state distinct from "failed" ("a failed,
cancelled, unverified, or policy-violating run is never marked
restorable"; job states "queued, running, verifying, sealing, succeeded,
failed, cancelled, quarantined"). Without the check above, an operator who
explicitly cancelled a job would see a generic "capture failed, check your
SSH preflight" message — actively wrong.

### Excluding the child from the raw signal

A child spawned via `std::process::Command` inherits the parent's console/
foreground process group by default on both platforms, so an operator's
raw Ctrl+C reaches the spawned `ssh` process directly too, independently
of and racing the cooperative kill path. Not a correctness bug (the poll
loop already tolerates an already-exited child cleanly on its next tick),
but a messier shutdown sequence than intended. `SystemOpenSsh` now spawns
every child through a small `new_command()` helper that requests
`CREATE_NEW_PROCESS_GROUP` on Windows and a new POSIX process group on
Unix (both stable, safe `std` APIs — no new dependency, no `unsafe`, which
this workspace forbids outright) — so only the cooperative,
`cancellation`-checked kill path can end the child. Verified only as a
spawn-still-succeeds regression guard (no dependency exists in this
workspace to introspect OS-level process-group membership); the OS-level
guarantee itself rests on documented Win32/POSIX behavior, the same class
of platform assumption the existing mid-push kill test already names
explicitly as worth a dedicated check rather than trusting either way.

### CLI: `ctrlc`, wired once, covering both `plan` and `execute`

`guardian-cli deploy` is the only CLI-invoked operation that spawns and
waits on an SSH child at all (`restore execute` is purely local disk
extraction; there is no `capture`/`plan`/`job` CLI subcommand). Added
`ctrlc = "3"` (no `termination` feature — that covers SIGTERM/SIGHUP for
unattended-scheduling scenarios that don't exist in this product yet,
revisit if/when scheduling ships) as a new workspace dependency. `deploy`'s
dispatcher builds one `CancellationHandle`, installs the handler once
(`let _ = ctrlc::set_handler(...)`, not `.expect(...)` — this workspace
denies `clippy::expect_used`, and a failed second-handler-install is a
programming error worth degrading past silently rather than aborting an
otherwise-working command over), and both `plan` and `execute` share the
one `SystemOpenSsh` built with that handle.

### Desktop: the first shared Tauri app state

Confirmed zero shared Tauri state existed before this change — every
command opened fresh stores per invocation. A new `JobRegistry`
(`Mutex<HashMap<RunId, CancellationHandle>>`, managed once via
`.manage(...)`) tracks in-flight capture/deploy jobs by run id. Its
registration guard is RAII (`Drop` unregisters), mirroring
`guardian-local-repository`'s existing `ProcessLock` pattern deliberately:
this is the first time any of this app's `spawn_blocking` call sites
registers something in shared state that must be released regardless of
how the job ends, and a manual trailing `unregister()` call could be
bypassed by a future early return in a way `Drop` cannot. A new
synchronous `cancel_job(run_id) -> bool` command looks the id up and
flips its flag — no `spawn_blocking` needed, unlike the jobs it cancels.

Because a Tauri command is a single request/response with no event stream
back to the frontend, the frontend cannot learn a server-generated run id
while the original `run_capture_plan`/`execute_deploy` call is still
pending. Rather than introduce Tauri's event system for the first time in
this codebase, the run id is now **frontend-supplied**
(`newRunId()` — UUIDv7, 36 characters, hyphens allowed, comfortably under
`RunId`'s 64-character ASCII-alnum-or-`-`/`_` limit) and threaded straight
through to the same audit-trail identifier `write_capture_audit`/
`write_deploy_audit` already use, replacing what was previously generated
server-side. A reused id fails closed — confirmed directly, not assumed:
capture's real collision gate is the `"started"` audit write (`write_new`'s
`create_new(true)`), which fires before `begin_staging` is ever reached;
deploy has no staging step at all, so its `"attempted"` audit write is its
only collision gate. Either way this is an *emergent* property of already-
existing infrastructure, not a purpose-built id-uniqueness check — named
here so a future reader doesn't have to rediscover it.

Follow-up on 2026-07-17 closes the correlation-id gap: desktop now produces
UUIDv7 in one shared helper and Rust-created IDs use `RunId::new()` backed by
the `uuid` crate's v7 generator. The MCP protocol deliberately continues to
accept caller-supplied, syntax-validated IDs so an external client can cancel
an in-flight request; adapters do not mint ad-hoc IDs for it.

## Consequences

- Real, operator-triggered cancellation exists for capture and deploy, on
  both the CLI and desktop, closing the last 0% item in Milestone 2's exit
  gate.
- `docs/ARCHITECTURE.md`'s cancellation/event-queue claims are corrected to
  match reality.
- One real, named gap remains: local restore extraction has no cancellation
  path yet (different mechanism, deferred to a follow-up that can reuse
  `CancellationHandle` directly).
- The clean-room drill now runs real mid-transfer capture and deploy
  cancellation: test-only forced-command fixtures throttle each filesystem
  stream only after its first byte, then the test cancels through the real
  `JobRegistry`. They prove the `cancelled` audit terminal state; capture
  leaves no local staging or sealed backup, while deploy leaves no target or
  remote staging directory. CLI Ctrl+C-specific wiring remains unproved end
  to end.
