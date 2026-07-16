# ADR 0007: Remote deploy — push a sealed backup onto a new/clean VDS

## Status

Accepted.

## Context

The product's stated golden path is: connect → backup → store → deploy on a
server. Every other stage works today; deploy does not exist at all. Local
restore (`RestorePlan`, `LocalRepository::execute_restore`) only extracts a
sealed backup to a path on the *same machine running Guardian*. "Deploy to a
new/different remote VDS over SSH" has exactly one four-word mention anywhere
in the docs (`docs/DEVELOPMENT_PLAN.md`, "New-host bootstrap assistant with
explicit prerequisites," under Milestone 4) and zero prior design.

This is the single most security-sensitive feature in the project to date.
`AGENTS.md`/`CODEX.md` flag this exact combination explicitly: SSH command
execution together with "restore planning or remote mutation" require a
SECURITY_MODEL.md update, adversarial tests, and — per the project's
source-of-truth rule — a new ADR. `CODEX.md`'s standing invariant applies
directly: *"Destructive server mutations require an explicit plan, scope
preview, typed confirmation, audit record, and a fresh pre-restore backup
unless waived by a recorded break-glass decision."*

**Scope of this slice**: push a sealed backup's filesystem payload (and
database payload, if present) onto an empty/absent path on a *different*,
separately-enrolled, host-key-pinned `VdsProfile`. CLI-only — no desktop GUI
wiring yet, matching how the embedded-database slice landed adapter-first.
The operator provisions a fresh VDS and enrolls it exactly like a capture
source, then picks that profile as the deploy target; no new profile schema.

## Decision

### Atomic rename, not a bare guard-then-extract

A bare `[ ! -e path ] || exit 1; tar --extract ...` has a real problem: if
the stream is interrupted mid-extraction (network drop, remote disk full),
the target is left non-empty but incomplete, and a retry fails the same
guard with no path back. Unlike capture's "delete the partial local file on
failure" (trivial: same process, same machine), a failed deploy's partial
state is on a third-party remote host, and any cleanup attempt needs a
second SSH round-trip that can fail for the identical underlying reason.
Both push commands (`guardian-ssh`'s `push_filesystem_command`/
`push_database_command`) instead extract/write into a freshly created,
uniquely named sibling temp path (via `mktemp -d`/`mktemp`, never a fixed
guessable name — see the 2026-07-16 amendment below) and atomically rename
into place only on success, using `mv -n` (no-clobber) plus a post-move
absence check on the temp path — not a bare `mv` — so a value that raced
into the target during the transfer is never silently clobbered. The remote
shell's own control flow does the cleanup; no second connection is needed,
because `tar`/`zstd` detect a truncated stream and exit non-zero on their
own.

The database push guards `<path>/database.sqlite` specifically, not `<path>`
itself — the filesystem push (when present) already legitimately created
`<path>` first, so the two pushes cannot share one guard.

**Residual risk**: "target verified absent" is a check against a remote
filesystem outside Guardian's exclusive control, not a guarantee about the
instant of the rename. The mitigation bounds the *damage* (never clobbers
what's really there) — it does not eliminate the race. A second residual
risk on the write side: without a freshness/generation counter, if the
remote host itself crashes before its own cleanup trap runs, an orphaned
sibling temp path can be left behind (never a partial target, since the
rename is the only way content reaches the real path).

### `RemoteTargetPath`: a newtype, breaking from an existing raw-`String` precedent

`valid_remote_root`-shaped validation already exists three times as a bare
`Vec<String>` + ad hoc validator (capture roots, in `guardian-core` and
`guardian-ssh`) — "raw validated string" is the established convention for
remote-path-shaped values. This is the first time such a value plays the
*same structural role*, side by side, as a local `PathBuf`
(`RestorePlan.destination: PathBuf` next to `DeploymentPlan.target_path`) —
exactly the mix-up a newtype should prevent. `RemoteTargetPath` (in
`guardian-core::identifiers`) requires an absolute POSIX path and — unlike
the capture-root validator, which explicitly allows bare `/` as a capture
source — rejects bare `/` outright, since a deploy target must be a path
that does not already exist, and `/` always does.

This also avoids a live cross-platform bug: this project runs its CLI on
both Windows and Linux, and `--target-path` names a POSIX path *on the
remote Linux VDS* regardless of which OS `guardian-cli` itself runs on.
Parsing it via `PathBuf::is_absolute()` (as `restore.rs`'s genuinely local
`--destination` correctly does) would silently apply the host OS's own path
semantics — wrong on Windows, where `is_absolute()` means "has a drive
letter," to a value that is never local. `--target-path` is validated only
through `RemoteTargetPath::parse`, at argument-parsing time, never through
`PathBuf`.

### Confirmation phrase and the self-overwrite guard

`format!("DEPLOY {backup_id} TO {target_profile_id}:{target_path}")` — the
same deterministic, no-randomness shape as `RestorePlan`'s
`"RESTORE {backup_id} TO {destination}"`. The security-critical fields stay
*inside* the compared string (not just displayed alongside it), so a stale
copy-paste from a different plan fails the equality check; the human
-readable profile label is deliberately left out of the compared string
(labels allow parens/colons, which would be visually confusing nested inside
the phrase) and shown as its own JSON field (`targetProfileLabel`) next to
`confirmation` instead.

`DeploymentPlan::build` blocks deploying a backup onto its own source,
checking **both** identifiers recorded on the manifest's `SourceIdentity` —
`profile_id` (catches re-selecting the same profile) and
`host_key_fingerprint` (catches re-enrolling the identical physical host
under a second, differently-named profile — the more realistic accident).
`host_key_fingerprint` is now a shared `guardian_core::host_key_fingerprint`
function (relocated from a private one-liner previously duplicated only in
the Tauri desktop layer). This slice hard-blocks on either match, with no
override flag; an explicit override is named as deferred future work below.

### Audit record: a new `LocalRepository` method, not `AuditPort`

`AuditPort` (`guardian-core::audit`) has exactly one method,
`capture_failed`, and zero production implementations anywhere in the
codebase — it exists purely as a control-flow hook so a capture use case can
trigger `storage.discard()` on failure, not as the actual audit trail. The
real persisted trail is `LocalRepository::write_capture_audit`, called
directly by the Tauri layer around composition calls, writing atomically
-named JSON under `<repository>/audit/`. Deploy has no local staging to
discard into, so a parallel `LocalRepository::write_deploy_audit(run_id,
state, backup_id, target_profile_id)` was added instead, mirroring
`write_capture_audit`'s shape exactly (`deploy-{run_id}-{state}.json`, one
record per state transition, `run_id` generated fresh per CLI invocation so
retries of the same backup/target pair never collide with or silently
overwrite a prior attempt's record). `DeploymentComposition::execute`
itself now writes the full `attempted` → `completed`/`cancelled`/`failed`
audit trail (see the 2026-07-16 audit amendment below) — every caller,
`guardian-cli/src/deploy.rs` included, simply invokes it and reacts to the
result.

This satisfies CODEX.md's "fresh pre-restore backup" clause vacuously
(nothing was there to back up) — that reasoning does not extend to the
audit-record requirement in the same sentence, which is not conditioned on
the break-glass waiver.

### Fresh-per-payload re-verification — a deliberate divergence from `execute_restore`

`execute_restore` loads and verifies the manifest once and reuses it for
both extractions, because both are fast, local, back-to-back operations.
Deploy's two pushes are network-bound (the existing 15-minute
`total_timeout` precedent) — a materially larger window during which the
sealed backup could be tampered with between the first push finishing and
the second starting. `LocalRepository::open_deploy_payload_reader` re-runs
manifest verification fresh on *every* call, and
`DeploymentComposition::execute` calls it once per payload, immediately
before that payload is pushed — never once for the whole operation. Like
`execute_restore`, it takes primitives (`backup_id`, `payload_path`,
`verifier`, `secrets`), never a `DeploymentPlan` value, so nothing ever
trusts a plan object as a trust anchor from its caller.

### Push mechanism (`guardian-ssh`): a new pump, reusing the direction-agnostic waiter unchanged

`wait_for_stream` only touches `Child`/`Receiver<()>`/`AtomicBool`, never
`stdout`/`File` directly, so it is reused unchanged for the push direction.
A new `PushPump` mirrors the existing pull-side `CapturePump`: it reads a
local, boxed `Read + Send` source in a background thread and writes into a
`ChildStdin`, pinging the same activity-channel protocol after each
successful write and setting the same shared `AtomicBool` on any error. The
source is boxed (`Box<dyn Read + Send>`, not a concrete `File`) because the
real source — a decrypted-payload reader from `guardian-local-repository` —
may need to keep a scratch-file guard alive alongside the readable handle,
something a bare `File` cannot represent; the public `push_filesystem_to`/
`push_database_to` methods accept `impl Read + Send + 'static` and box it
internally, so callers never see the boxing.

The push pump takes `expected_bytes: u64` — measured directly from the
decrypted content `open_deploy_payload_reader` actually produces, never
taken from `PayloadEntry.byte_length` (an earlier version of this mechanism
used that manifest field directly; see the 2026-07-16 amendment below for
why that was wrong) — and fails closed on an **exact** mismatch — too few
bytes at EOF, not just too many mid-stream — stronger than the pull side's
ceiling-only check, and it catches a local scratch-file truncation the
ceiling alone would miss. EOF is signaled by explicitly dropping `ChildStdin`
once the source is exhausted, on every return path (success, mismatch, or
I/O error) — that close is what lets the remote command observe EOF and
exit instead of blocking forever on more input.

A cross-platform adversarial test (`killing_a_stalled_remote_mid_push_does_
not_hang_the_pump`) verifies that killing a stalled remote mid-push does not
hang the pump thread on either Windows or Linux — Unix `write()`-into-a
-closed-pipe behavior on `child.kill()` is well understood, but the
equivalent Windows `TerminateProcess`-vs-pending-`WriteFile` interaction was
verified explicitly rather than assumed.

### Orchestration: a new sibling crate `guardian-deploy`

`guardian-capture`'s Cargo.toml already depends on both
`guardian-local-repository` and `guardian-ssh`, so nothing in the dependency
graph forces a new crate — the reason is reviewability. For the project's
most security-sensitive feature, "everything capable of mutating a remote
server" should be enumerable from one crate's directory listing, not mixed
into a crate whose name and doc comments promise read-only, pull-direction
behavior. This also costs no real duplication: the profile-to-(host, user,
identity) resolution block was already copy-pasted between
`FilesystemCaptureComposition` and `EmbeddedDatabaseCaptureComposition`,
both in `guardian-capture` — keeping deploy there would not have saved that
duplication either. `guardian-vault` (ADR 0006) is direct precedent for "new
distinct capability gets its own crate."

`DeploymentComposition::plan` touches the network once (a target-absence
preflight probe) to give the operator early feedback before they ever type
the confirmation phrase — a deliberate divergence from restore's fully
-offline `plan_restore`, since deploy's plan step is only useful if it can
say something about the destination. `DeploymentComposition::execute`
re-derives the plan from scratch internally and never accepts one as
trusted input, mirroring `execute_restore`'s own discipline.

## Alternatives rejected

- **Reusing `restore`'s CLI verbs** (`restore plan|execute` with a "remote"
  mode): `restore`'s `--destination` flag is already documented and tested
  as an absolute *local* path; overloading its meaning for a different
  action risks real confusion. A separate `deploy` subcommand keeps both
  contracts unambiguous.
- **Extending `RestorePlan` with a destination enum** (local path vs. remote
  profile+path): forces a local-only type (`PathBuf`) and a remote-only
  concept (target profile) into one struct for two different consumers.
  `DeploymentPlan` duplicates `RestorePlan::build`'s small (~15-line)
  payload-selection logic via a shared `select_payloads` helper instead —
  cheaper than the coupling a shared plan type would introduce.
- **`scp`/`sftp`/an SSH client library** instead of piping through the
  existing reviewed OpenSSH argv invocation: would introduce a second
  code path for host-key pinning, timeouts, and argument construction,
  duplicating (and risking drift from) `arguments_for_command`'s already
  -reviewed flag set. Piping a stream into a fixed, reviewed remote command
  over the same `SystemOpenSsh` invocation keeps one reviewed code path for
  every remote command this project runs, pull or push.

## Amendment (2026-07-16): three P0 correctness/security bugs fixed

A code review found three real bugs in the mechanism above, all confirmed
against the running code before this amendment was written.

**The `expected_bytes`/`byte_length` premise above was wrong for every
encrypted payload.** The original text said "since `PayloadEntry.byte_
length` is already verified against the signed manifest before the reader
is ever opened, the push pump takes `expected_bytes: u64`" — implying that
field was always the right count to hand the pump. It is not: `byte_
length` records the payload's on-disk stored size, which for an encrypted
payload is the *ciphertext* size (plaintext plus a fixed envelope header
and one AEAD tag per chunk, per `guardian-encryption`) — always strictly
larger than the plaintext byte count `open_deploy_payload_reader` actually
produces after decrypting. Passing the ciphertext count as `expected_
bytes` made `copy_stream_to_child`'s exact-match check fail with `ByteCount
Mismatch` on every encrypted deploy — after the remote side had already
received the complete, valid stream and successfully renamed it into
place, since the pump closes `ChildStdin` before evaluating the mismatch.
Every real capture path encrypts (confirmed via `LocalRepositoryStorage
Adapter::encrypted` being the only constructor any composition root ever
calls), so this was not an edge case — every encrypted deploy failed,
after the remote mutation had already fully succeeded, and any retry was
then blocked by the target now existing. Fixed by measuring the actual
decrypted byte count `open_deploy_payload_reader` is about to produce
(`DecryptedPayload::measured_len`, a cheap `.metadata()` call on the
already-open, already-decrypted file handle) and returning it alongside
the reader, rather than trusting a value recorded through an unrelated
historical code path. `byte_length`'s own meaning and its existing
`verify_payload_tree` integrity check are both untouched — this is a new,
independent measurement, not a reinterpretation of that field.

**The push commands unconditionally deleted a predictable sibling path.**
Both `push_filesystem_command` and `push_database_command` built a fixed,
guessable name (`"$target.guardian-deploy-tmp"`) and ran `rm -rf`/`rm -f`
on it before ever creating anything — deleting whatever already happened
to be there, with no check, no confirmation tie-in (the operator's typed
phrase never names this synthesized path), and no dry-run. Fixed by
replacing the fixed name with `mktemp -d`/`mktemp` (already an assumed-
present dependency elsewhere in this file), which names and creates a
fresh, unique entry in one atomic step — removing the need for any
unconditional pre-emptive delete entirely. The mktemp call is placed as a
sibling inside the same parent directory as the target (required so the
later `mv` stays a same-filesystem rename, never a silent cross-filesystem
copy), after the existing absence check and after `mkdir -p` on that
parent.

This closes the security bug at the cost of an incidental property the
original "Residual risk" note above didn't call out because the old
scheme's determinism was never the point: under the old, fixed name, a
crash before the remote's own cleanup trap ran left an orphan with a
*predictable* name a later retry's next `rm -rf` would have opportunistically
swept up. Under `mktemp`'s randomized suffix, a crash-orphaned temp entry
has an unpredictable name and is no longer self-cleaned by a later retry.
This is a deliberate, accepted trade-off — closing an unconditional-delete
vulnerability is worth losing an incidental self-healing side effect that
was never a designed guarantee — stated plainly here rather than left for
a future reader to notice as an unexplained regression.

**Remote extraction had no explicit ownership/permission policy.**
`push_filesystem_command`'s `tar --extract` invocation passed neither
`--no-same-owner` nor `--no-same-permissions`. GNU tar's undocumented-by-
this-code default is to restore archived ownership when run as root (and
never when not, regardless of any flag) and to always restore full
archived permission bits — including setuid/setgid — regardless of
privilege, unless told not to. Whether ownership restoration actually
happened therefore depended entirely on the incidental privilege of
whichever account ran the SSH session, never on an explicit policy this
code enforced. Fixed by adding `--no-same-owner --no-same-permissions`,
matching the policy `docs/SECURITY_MODEL.md` already states for local
restore's separate Rust-native extractor, extended explicitly to cover
this remote path too. The now-inert `--numeric-owner` flag (it only
affects ownership-restoration *display*, and does nothing once ownership
restoration itself is disabled) was dropped in the same change.

Not addressed by this amendment, named explicitly rather than silently
left: assembling the filesystem and optional database payload under one
remote staging root with a single final rename (today they are still two
independently atomic pushes — a failed second payload leaves a live,
partially deployed target with no database file — closed by the
2026-07-16 "staged deploy protocol" amendment further below), and making
attempted/completed/failed audit persistence part of this composition
itself rather than a responsibility duplicated by every caller (closed by
the audit amendment immediately below). Both were tracked in
`docs/DEVELOPMENT_PLAN.md`.

## Amendment (2026-07-16): audit persistence moved into the composition

The second non-goal named directly above is now closed: `DeploymentComposition::
execute` writes its own `attempted`/`completed`/`cancelled`/`failed` audit
trail unconditionally, reversing the original decision (further up this
document, "Audit record: a new `LocalRepository` method, not `AuditPort`")
that audit-writing was the caller's responsibility. That reversal is a
deliberate strengthening, not a correction of an error: nothing about the
original design was wrong, but leaving audit persistence to convention
meant a third caller — a future scheduler, a test exercising `execute`
directly, anything that didn't copy the existing wrapping exactly — could
silently skip it, which `CODEX.md`'s "destructive server mutations require
... an audit record" does not allow as an option.

Mechanically: `execute` gained a `run_id: &RunId` parameter (callers
already minted one for their own now-removed wrapping, so this only moves
where it's consumed, not who produces it) and a new private `write_audit`
helper wrapping `LocalRepository::write_deploy_audit`. Cancellation
detection moved with it: `SystemOpenSsh` gained a small `is_cancelled()`
accessor so the composition can distinguish `"cancelled"` from `"failed"`
using the same `CancellationHandle` its own `ssh` field was already built
with, without a second copy of that handle threaded through separately.
The existing strict/best-effort split is preserved exactly: `"attempted"`/
`"completed"` writes are strict (a write failure here fails the call even
though the push itself succeeded), `"cancelled"`/`"failed"` are best-effort.

`FilesystemCaptureComposition::execute` (`guardian-capture`) gained the
identical treatment in the same change, for the identical reason — it is
not part of this ADR's own scope, but is named here because both
compositions now share one pattern deliberately. It needed no new
parameter: `FilesystemBackupRequest.capture.run_id` was already available
inside the request `execute` already takes.

Named explicitly, not silently carried forward: `EmbeddedDatabaseCaptureComposition`
(`guardian-capture/src/embedded_database.rs`) is a third, structurally
identical composition with the same audit gap. It has no production caller
today, so it is untouched by this amendment — but it would silently lack a
real audit trail the moment something calls it directly, and whoever wires
it up next should give it the same treatment rather than rediscovering this
gap.

## Amendment (2026-07-16): staged deploy protocol closes cross-payload atomicity

The first non-goal named several paragraphs above is now closed: a failed
database push can no longer leave a live, partially deployed target on the
remote host. Local restore's identical bug was fixed first (a separate
crate, `guardian-local-repository`) by staging both payloads under one
local sibling directory and doing a single `fs::rename`; deploy's version
is structurally harder because each payload push is its own separate SSH
round-trip — no local call can span a rename across two independent
process spawns the way a single filesystem call can.

**A filesystem-only deploy needed no change at all.** `DeploymentPlan.
database_payload: Option<PayloadPath>` is `None` whenever the sealed
backup has no database payload, and the existing single push-then-rename
(`push_filesystem_command`) is already fully atomic in that case — there
is no second payload to race against. Every change below lives behind
`execute_pushes`'s `Some(database_payload)` branch.

**For a combined deploy**, two independently atomic pushes were replaced
with stage → stage → one finalize rename (three remote round-trips): the
filesystem payload extracts into a shared staging directory without
renaming (`push_filesystem_into_staging_to`); the database payload writes
into `<staging>/database.sqlite`, also without renaming
(`push_database_into_staging_to`); a new `finalize_deploy_to` does the
one rename that publishes both payloads at the real target atomically.
The old direct-to-target `push_database_to`/`push_database_arguments`/
`push_database_command` were deleted outright — a database payload never
exists without a filesystem payload (`select_payloads` requires exactly
one filesystem payload), so the old direct-to-target database push was
never reachable once staging landed.

**The staging directory's name is chosen on the Rust side, not by remote
`mktemp`**: three separate SSH invocations need to agree on one path, and
reading a `mktemp`-chosen name back over stdout would require changing
`push_to`'s `Stdio::null()` stdout handling. Instead, the name is
`<parent>/.guardian-deploy-staging.<run_id>` — `RunId` (already threaded
through `execute()` for the audit amendment above) is fresh per deploy
attempt and its validator (`guardian-core::identifiers`) restricts it to
ASCII alphanumeric plus `-`/`_` only, so it can be embedded directly with
no `shell_quote` escaping, unlike `target_path` (arbitrary POSIX path
text, always quoted). This closure depends on callers minting high-entropy
run ids — both current callers do (CLI's `OsRng`-backed `random_run_id()`,
desktop's `crypto.randomUUID()`) — but `RunId::parse` itself only checks
charset and length, not entropy; that is a caller convention being relied
on, not something the type enforces, named here rather than left implicit.

**Cleanup stays entirely inside each remote script**, matching this ADR's
existing philosophy ("the remote shell's own control flow does the
cleanup; no second connection is needed"): a Rust-side cleanup call after
a failed database push was considered and rejected, since it would itself
be a mutating remote operation that can fail for the identical reason the
thing it's cleaning up after failed. `push_database_into_staging_command`
cleans up the *entire* staging tree on its own failure, not just the one
file it was writing, since a failed second stage abandons the whole
attempt including the first stage's already-staged content.

**Residual risk, alongside the existing mktemp-orphan disclosure above**:
if the SSH session drops entirely between stages (a real gap of possibly
minutes, matching this ADR's own "fresh-per-payload re-verification"
reasoning for why the two pushes aren't done back-to-back trustingly), the
orphaned staging directory is never cleaned automatically. This is
accepted, not overlooked: a later deploy attempt mints its own fresh
`run_id`, so it gets a differently-named staging directory and can never
collide with the orphan — inert garbage, not a correctness risk. The
exposure window is now bounded by up to two extra SSH round trips instead
of one continuous script, a quantitatively larger version of the same
residual risk class, not a new kind of one.

**Test coverage, honestly bounded**: the three new remote command
templates are covered by the same string-assertion convention every
template in this crate already uses (`crates/guardian-ssh/tests/
pinned_deploy.rs`), including a dedicated test proving all three agree on
the same staging path for one `run_id`. Simulating "the filesystem push
succeeds, the database push then fails" at the unit level is not
realistically possible here — `DeploymentComposition.ssh` is a concrete
`SystemOpenSsh`, and the existing missing-ssh-binary test trick fails
every push identically, unlike local restore's equivalent test, which
could selectively deny just one payload's key via a custom `SecretStore`.
The clean-room drill (`deploy_drill`) already deploys a combined backup
end to end and remains the only realistic proof of the staged protocol's
cross-payload behavior; deliberately failing the second push mid-drill
would be a valuable future addition to that harness, not built in this
slice.

## Consequences

A backup can now be pushed onto a new/clean VDS end to end (plan → typed
confirmation → push, with a persisted audit trail), closing the last major
gap against the product's own stated golden path.

Explicitly not delivered by this slice, and not blocking it: diff/dry-run
file-level preview, staged switch-over, rollback, signed report, service
stop/start orchestration, database-aware live cutover, cross-version format
compatibility, desktop UI, an explicit same-host override flag, and
consolidating the three pre-existing `valid_remote_root`-shaped duplicates
into a shared type now that `RemoteTargetPath` exists as a precedent.
