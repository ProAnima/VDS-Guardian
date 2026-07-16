# Development Plan

VDS Guardian is intentionally a small backup-and-restore application. Its first
release has one job:

```text
connect to one Linux server
        -> capture explicitly selected data
        -> store one verified backup on a local/removable disk
        -> restore that backup to a new local or remote destination
```

The roadmap is ordered around proving that path. New discovery, automation, or
platform features do not enter the current release unless they remove a blocker
from this exact flow.

## Product boundary for the first release

Included:

- Windows and Linux desktop application backed by one shared Rust engine;
- a scriptable CLI for the same essential operations;
- pinned OpenSSH connection to a remote Linux server;
- operator-selected absolute filesystem paths;
- streaming tar.zst capture into a local or removable repository;
- optional SQLite snapshot stored in the same backup transaction;
- encrypted, versioned, checksum-verified backups;
- explicit verification, listing, local restore, and deploy to a new remote
  destination;
- cancellation, bounded resource use, and actionable failures;
- one automated clean-room backup-and-restore drill.

Not required for the first release:

- automatic Docker or Compose inventory;
- PostgreSQL or MySQL dump/restore adapters;
- arbitrary quiesce hooks;
- in-place restore of an existing live server;
- automatic retention, replication, cloud/object storage, or deduplication;
- native scheduling, background services, notifications, or auto-update;
- organization policies, approval workflows, Kubernetes, or malware scanning.

Existing implementations outside the first-release boundary may remain in the
repository, but they are frozen except for correctness or security fixes. They
must not expand the release gate or create a second orchestration path.

## Completed foundation

The repository already contains more foundation than the first release needs:

- Rust core shared by Tauri and CLI surfaces;
- validated profiles, plans, identifiers, and manifest versions;
- pinned system-OpenSSH transport with timeouts, limits, and cancellation;
- isolated staging, SHA-256 verification, quarantine, and atomic seal;
- Ed25519 manifest signing and OS credential-store integration;
- AES-256-GCM chunked payload encryption;
- safe tar.zst inspection and new-destination extraction;
- local repository locking and interrupted-staging recovery;
- filesystem plus optional SQLite capture;
- local restore and new-host deploy foundations;
- Windows/Linux canonical gates and Linux Docker integration profiles.

These components are implemented or locally tested, but their existence does
not make the product ready. Readiness is decided only by the release path and
exit gate below.

## Release 0.1 — reliable manual backup and restore (P0, current)

Work is completed in this order. Later items do not justify skipping an earlier
gate.

### 1. Repair the current restore/deploy transaction

- Compare pushed plaintext against its authenticated plaintext length, not the
  encrypted payload length stored in the manifest. Closed: the push path now
  measures the actual decrypted byte count instead of trusting the manifest's
  on-disk stored-size field, which is strictly larger whenever a payload is
  encrypted.
- Replace deterministic remote temporary paths and unconditional cleanup with a
  create-new, per-run staging path owned by that operation. Closed: both push
  commands now name and create their sibling temp path via `mktemp`/`mktemp
  -d` in one step instead of unconditionally `rm -rf`/`rm -f`-ing a fixed,
  guessable name before use.
- Enforce an explicit ownership and permission policy during remote extraction;
  never restore setuid/setgid bits or archive-provided owners by default.
  Closed: the remote `tar --extract` invocation now passes `--no-same-owner
  --no-same-permissions` explicitly, matching the policy already stated for
  local restore's extractor.
- Assemble filesystem and optional database payloads under one staging root and
  publish them with one final rename. A failed second payload must not leave a
  partial destination. Closed for local restore: both payloads now extract
  into one sibling staging directory and publish with a single rename,
  guarded by a fresh existence check immediately before it. Still open for
  deploy — its two payloads are pushed as independently atomic remote
  renames, so a failed second push still leaves a live, partially deployed
  target; closing this needs a two-phase remote staging protocol, not a
  local rename, and is deliberately its own separate slice.
- Make attempted/completed/failed audit persistence part of the mutating
  application use case rather than a responsibility duplicated by CLI and
  Tauri callers. Closed: both `DeploymentComposition::execute` and
  `FilesystemCaptureComposition::execute` now write their own audit trail
  unconditionally; every caller (CLI deploy, desktop deploy, desktop
  capture) dropped its own duplicated wrapping.
- Add regression tests for each failure mode above. Covered for four of the
  five closed items above; not yet for deploy's still-open atomicity bullet.

Gate: a failed or cancelled restore/deploy leaves no published partial target,
does not delete a path it did not create, and records an accurate terminal
state. Met for local restore and for the audit trail on both restore and
deploy. Not yet met for deploy's own atomicity — a failed second payload
push still leaves a live, incomplete target on the remote host.

### 2. Make encrypted backups independently recoverable

The OS credential store is useful working storage but cannot be the only copy
of payload keys. Losing the operator machine must not make an intact backup
disk unreadable.

- Decide and document a portable recovery-key model in an ADR that supersedes
  the local-keyring-only consequence of ADR 0004.
- Wrap per-backup data keys with a repository recovery key.
- Provide an explicit encrypted recovery bundle and a documented offline-copy
  procedure; ordinary settings export must still contain no plaintext secret.
- Prove restore on a clean operator machine that has the repository and recovery
  bundle but no original OS credential-store state.
- Keep key import/export explicit, confirmation-gated, and testable from CLI.

Gate: a clean machine can verify and decrypt an existing backup using only the
documented recovery material, while a missing or incorrect recovery key fails
closed.

### 3. Finish one shared application workflow

- Expose capture, verify, list, restore, deploy, and cancel through one
  application-service boundary shared by CLI and Tauri.
- Add the missing CLI capture entrypoint so desktop and headless operation do
  not have different backup capabilities.
- Keep Tauri commands and CLI parsing thin: they validate DTOs, invoke one
  operation, and map one typed result.
- Return bounded progress states and stable error/remediation codes.
- Keep filesystem paths explicit; Docker discovery is optional input assistance,
  not part of capture correctness.

Gate: the same fixture plan produces the same sealed backup and restore result
when triggered through CLI and desktop adapters.

### 4. Complete the operator path

- The desktop setup flow covers repository, server profile, selected paths, and
  recovery-key readiness without exposing internal architecture.
- Backup list shows sealed/failed/cancelled state and last verification result.
- Restore preview states the source backup, destination, expected writes, and
  rollback posture before confirmation.
- Failures tell the operator what is safe, what may have changed, and what to do
  next.
- Produce signed Windows and Linux installer artifacts with published
  checksums; automatic updates remain deferred.
- Documentation and UI use the same release status and terminology.

Gate: a non-technical operator can configure one server, create one backup,
verify it, and restore it to a new destination without editing JSON or reading
an ADR.

### 5. Prove the release in a clean room

- Run the compiled SSH, repository, encryption, capture, and restore/deploy
  adapters against disposable source and destination hosts.
- Test filesystem-only and filesystem-plus-SQLite backups.
- Cover cancellation, corrupted payload, missing recovery key, disk exhaustion,
  changed host key, hostile archive metadata, and failure of the second payload.
- Record byte/data integrity, elapsed time, and cleanup state.
- Run the drill from the same commit on Linux CI; keep Windows canonical gates
  green and perform a documented Windows desktop smoke test.

Exit gate for 0.1: a clean machine with documented recovery material restores a
backup to a clean destination, verifies the expected filesystem and SQLite
data, and proves that every exercised failure leaves no published partial
target. Until this gate passes, the project is not a disaster-recovery system.

## Release 0.2 — routine operation (P1)

Only after 0.1 is proven:

- native scheduling through the existing CLI;
- simple whole-backup retention with preview and confirmation;
- job history, diagnostics export, and local notifications;
- signed updater metadata, SBOM, provenance, and upgrade documentation;
- key rotation and recovery-bundle replacement;
- accessibility and long-running desktop polish.

Exit gate: scheduled backups and retention cannot bypass the same application
use cases or weaken the manual 0.1 restore proof.

## Release 0.3 — discovery and additional workloads (P2)

Candidates, selected by real operator demand:

- Docker/Compose inventory and mount selection;
- PostgreSQL and MySQL dump/restore adapters;
- versioned application quiesce adapters;
- richer health checks and restore reports;
- repository replication or S3-compatible storage.

Each workload adapter needs its own capture-consistency contract and clean-room
restore fixture. Discovery metadata alone never counts as recoverability.

## Development stop rules

- No new first-release feature while an earlier 0.1 gate is red.
- No new crate solely to hold one adapter or one DTO group; prefer a module
  until an independently versioned security boundary is demonstrated.
- No duplicate orchestration in CLI, Tauri, tests, or schedulers.
- No production-readiness claim based only on compilation, unit tests, or a
  successful backup. Restore evidence is mandatory.
- Documentation drift is a failed gate: README, this plan, architecture, and
  UI status must describe the same implemented state.
- Module and function budgets remain split signals. Security-sensitive modules
  over budget are decomposed while related behavior is being changed, without a
  separate big-bang rewrite.

## Definition of production-ready

For the scoped release, production-ready means:

- all Release 0.1 gates pass;
- payloads are encrypted and independently recoverable;
- SSH identity, command, path, archive, and filesystem boundaries fail closed;
- Windows and Linux canonical verification pass from the same commit;
- the clean-room drill passes with the compiled production adapters;
- installer artifacts are signed and accompanied by checksums;
- limitations and recovery-key handling are documented for an operator.

Features deferred to 0.2 or 0.3 are not hidden production blockers. They remain
out of scope until deliberately promoted into a release.
