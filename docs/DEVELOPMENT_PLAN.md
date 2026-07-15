# Development Plan

The order is safety-driven: format, state machines, and hostile-input tests land
before real remote mutation. Each milestone ends with a demonstrable gate.

## Iteration 0 — production foundation (completed)

Deliverables:

- repository, Apache-2.0 license, English public documentation;
- architecture, threat model, backup format draft, ADRs, agent contracts;
- Rust workspace with shared core, CLI shell, and Tauri/React shell;
- central `doctor` and `verify` commands plus Windows/Linux CI;
- typed foundation status contract and baseline unit tests.

Exit gate: a clean clone can install dependencies and pass `npm run verify` on
Windows and Linux. No claim of functional backup capability.

## Milestone 1 — domain and local repository (P0, current)

- Versioned profile, plan, manifest, job, policy, and audit schemas.
- Explicit backup/restore state machines with property tests.
- Atomic local repository adapter, locking, recovery of abandoned staging, and
  whole-directory retention.
- Canonical JSON, SHA-256 verification, signing identity, and golden fixtures.
- Hostile manifest/path/archive test corpus.

Exit gate: simulated byte sources create sealed independent backups; corruption,
interruption, traversal, and concurrent writers fail safely.

Implemented slice: validated identifiers and manifest fields, exhaustive state
transition tests, a separate local repository adapter, cross-process locking,
safe payload paths, streaming SHA-256 verification, Ed25519 signing identities,
Windows Credential Manager and Linux Secret Service integration, a byte-exact
format-v1 golden fixture, quarantine, abandoned-staging recovery, and atomic
directory seal. Whole-directory retention now re-verifies every sealed backup,
creates deterministic snapshot-bound dry runs, requires exact approval, and
records append-only audit evidence. A streaming tar.zst inspector now validates
paths, rejects links and special entry types, and enforces entry, per-file, and
expanded-stream limits. A deterministic tar.zst writer emits normalized archive
headers for validated paths. Full schemas and extraction coverage remain open:
no independent schema-definition artifact exists for any type (Rust struct
shape plus serde attributes is the only "schema" today), no job schema or
persisted job history exists at all (only capture-plan documents and audit
files), retention/audit records are not themselves version-tagged, and
extraction has materially thinner test coverage than inspection. An automated
clean-room restore drill now exists (`crates/guardian-capture/tests/
clean_room_drill.rs`, run via `npm run test:integration:drill`): it captures a
real filesystem-plus-database backup over a real SSH round trip against a
disposable container, seals it, restores it locally, and byte-verifies the
result — closing the "only a manual runbook procedure" gap for that one
fixture stack. It does not yet cover every supported stack type and does not
touch rollback, which is unbuilt (Milestone 4). The initial hostile archive path corpus now
pins a fail-closed cross-platform path contract. Retention now
records a durable, non-secret transaction intent: reopening rolls back a
partially moved deletion set, or resumes only a cleanup phase that was durably
approved.
Signing identity enrollment now uses a
cross-process lock, an atomic public configuration, and a non-secret recovery
intent. Read-only status and explicit enrollment are available through JSON CLI
and Tauri bridge commands; the desktop setup UI requires a separate explicit
acknowledgement before enrollment. Explicit rotation remains open.
Staging payloads can now be reserved exclusively for streaming adapters and are
registered only after a regular-file check and disk-based SHA-256 calculation.
A headless node without a usable OS credential store can now opt into
`guardian-vault` (ADR 0006), a local AES-256-GCM-CHUNKED-encrypted file store
implementing the same `SecretStore` contract, selected explicitly per
invocation via `--vault-dir` on `guardian-cli credential`/`restore`/`signing`;
`vault init`/`vault status` bootstrap and inspect it. Vault key rotation and a
credential migration tool between backends remain open.

## Milestone 2 — secure SSH capture (P0)

- OS keyring credential references and dedicated-key enrollment.
- Known-host pinning, changed-key failure, timeout/cancel/output limits.
- Read-only capability discovery and reviewed remote command templates.
- Streaming filesystem capture with deterministic metadata normalization.
- Integration tests against ephemeral SSH containers and a compromised-server
  fixture suite.

Exit gate: capture from a disposable Linux host is repeatable and cannot escape
the plan or repository under adversarial filenames/output.

Foundation implemented: `guardian-ssh` builds direct system-OpenSSH argv for a
validated user and a temporary exact pinned `known_hosts` entry, with strict
host-key checking, password/keyboard-interactive authentication disabled, and a
reviewed read-only tar capture template. It deletes a partial local stream when
OpenSSH cannot launch or returns failure. It resolves an enrolled profile
credential reference through the OS credential store into a short-lived local
identity file. Windows ACL hardening (already covering that identity file and
the vault) now also narrows the local capture destination the moment it is
created, before any streamed byte reaches it, and every file the local
repository writes through its shared atomic-write primitive — closing the
one gap found when this line was checked against current code: the staging
payload, holding arbitrary captured content in plaintext until encryption,
previously had no permission restriction on any platform. Capability
discovery remains narrow and single-purpose (tar/zstd and sqlite3 presence,
database version parity) rather than one unified host-capability report, but
now also covers the one concrete gap that mattered in practice: the
embedded-database snapshot command needs a remote scratch copy of the whole
database file, so a fixed read-only probe reports the remote host's free
space and the capture fails closed rather than mid-stream if it would not
cover the file. Filesystem capture needs no equivalent check — its `tar
--zstd` template streams to stdout with no remote scratch file. OS/distro
detection and a single aggregated capability report remain open for this
milestone's exit gate. Operator-triggered cancellation (ADR 0010) is now
implemented for capture: the desktop app's Cancel affordance and a per-job
registry signal a cross-thread handle the transport polls between reads,
and the spawned child is isolated into its own process group so only that
cooperative signal ends it.
Encrypted-key/agent support (ADR 0009) is now implemented: a passphrase-
protected key already loaded in an OS SSH agent is used through a `.pub`-
only identity file, resolved from a self-describing public-key marker
stored under the same credential reference — registered today via
`guardian-cli credential register-agent-key`, with no desktop UI yet and
no RSA identities in this first slice.

`guardian-capture` now connects any filesystem capture transport, including the
pinned OpenSSH transport, to an exclusive staging payload path. It inspects the
completed tar.zst stream before computing the disk-based digest. The live
composition accepts a full backup request and signer, then registers the
payload, finalizes the manifest, verifies the signature, and atomically seals
the staging directory as one fail-closed operation; any capture, reserve, or
finalization failure is audited and discarded/quarantined. The capture
composition derives the SSH target only from the matching validated pinned
profile. A shared preflight use case now loads that profile, invokes the
read-only `tar --zstd` probe, and blocks unsupported hosts before capture.
The resolved live composition invokes the same probe before staging, caps the
compressed stream at 20 GiB, and requires a 5 GiB free-space reserve beyond
that budget. Disposable-host integration tests remain open.

The reproducible Alpine OpenSSH fixture is available through
`npm run test:integration:ssh`; it verifies a real pinned-key capture and
changed-key rejection against a normal, well-behaved server — a distinct
compromised/adversarial-server fixture suite does not exist yet (only
scattered adversarial unit tests elsewhere cover hostile input), and that
fixture script itself still drives a hand-built `ssh` invocation directly
rather than the compiled `guardian-ssh` adapter. A separate clean-room drill
fixture (`npm run test:integration:drill`) now does connect real compiled
adapter code to a live network round trip: it drives the actual
`guardian-ssh`/`guardian-capture`/`guardian-local-repository`/`guardian-deploy`
composition roots against disposable containers for a real capture, local
restore, and remote deploy. This closes the "no test connects the real
adapter code to a live network round-trip" gap for that path specifically —
it does not add a compromised-server scenario, which remains open. CI
scheduling is done: the Linux CI job runs both Docker gates after the
canonical verification suite; Windows retains the same canonical suite
without requiring Docker.

## Milestone 3 — Docker and database consistency (P0)

- Docker/Compose inventory: files, project labels, image digests, networks,
  mounts, secrets references, and health state.
- Named-volume and bind-mount capture strategies.
- The initial product supports a lightweight embedded database only (SQLite or
  equivalent application-owned file) through an explicit application-consistent
  snapshot adapter. PostgreSQL/MySQL server dump and restore adapters are out
  of scope for the first release.
- Quiesce hooks are shipped, versioned application adapters; arbitrary remote
  scripts are never executed from captured content.
- Encrypted payload envelope ADR and implementation.

Exit gate: fixture stacks with databases restore to a fresh host and pass data
integrity and health checks.

Foundation implemented: `guardian-core` defines a bounded, validated Docker
inventory contract for container identity, image digests, Compose project labels,
mounts, networks, secret references, state, and health. It rejects duplicate or
unsafe metadata before an inventory is accepted. `guardian-docker` additionally
parses bounded `docker inspect` JSON into that contract and rejects unexpected
state or unsafe mount data. Its pinned-SSH adapter invokes only a reviewed
read-only Docker command, caps local output at 8 MiB, and passes it to the
parser. Named-volume and bind-mount capture strategies (ADR 0008) now exist:
a `DockerMount` resolves to a `capturable_path()` — a bind mount's already-
validated host path, or a named volume's host directory recovered from the
same `docker inspect` response's `Source` field the parser already fetched
but previously discarded — feeding directly into the existing filesystem-
capture mechanism with no new capture use case. Non-`local` volume drivers,
quiesce/consistency guarantees for a live volume, and any new privilege
beyond what the operator's own dedicated backup account already grants
remain explicitly open, not silently assumed solved. A minimal discovery
UI now exists (Milestone 5): the desktop capture-plan flow can list a
selected server's Docker containers and add a capturable mount's path as a
root with one click; a fuller inventory browser (image digest, health,
Compose project, secret references, networks — discovered but not yet
surfaced) remains open. Quiesce hooks as "versioned application adapters"
(the top-line scope item above) do not exist in any form: every capture is
a point-in-time snapshot with no per-application consistency hook, for
Docker mounts or otherwise, and only the embedded-database adapter's own
SQLite-specific `.backup` mechanism (below) provides an analogous guarantee
for that one case. Arbitrary remote scripts are never executed from
captured content, as originally stated — every remote command remains a
fixed, reviewed template regardless of what a capture contains.
Database preflight now requires matching major versions between a
reported PostgreSQL/MySQL server and its selected dump tool, and rejects an
empty discovery result. `guardian-database` composes the fixed server-version
and dump-tool probes into the core capability port, so an SSH preflight cannot
succeed with only one side of that comparison. It discovers locally available
`pg_dump`/`mysqldump` versions through a fixed,
pinned-SSH command with a 64 KiB output cap. An `sshPeer` database connection
can now use that same pinned VDS profile to request a server version from
`localhost` or `127.0.0.1` without passing a database password over SSH. The
remote commands are fixed `psql --no-password` or `mysql --skip-password`
version queries; unavailable local non-interactive database authorization fails closed. A
credential-reference connection mode remains modeled but has no adapter yet.
The lightweight embedded-database snapshot adapter now exists end to end
(ADR 0005): a fixed `sqlite3 .backup` plus `zstd` remote command produces a
consistent, WAL-safe snapshot of one operator-configured absolute database
file, gated by a narrow `sqlite3` presence probe. The captured stream is
encrypted like any other payload and validated by a new bounded zstd-stream
inspector. The desktop's capture-plan flow now offers an optional database
file path alongside the filesystem roots: a plan with one set captures both
into one sealed backup from a single run, since restoring or deploying a
database-only backup is not supported by this codebase (both require exactly
one filesystem payload) and unifying needed no change to either. The
standalone, independently-sealed database-only capture path remains
available in `guardian-capture` for potential future use, just not what the
desktop UI calls. Restore decrypts and zstd-decompresses the database
payload directly into `database.sqlite` at the restore destination,
alongside the filesystem payload. Fixture-drill evidence for a database
capture now exists (`npm run test:integration:drill`): the clean-room drill
seeds a real SQLite database in a disposable container, captures and
restores it locally with a byte-exact check, and separately deploys it to a
second disposable container where a real `PRAGMA integrity_check` and a
row-level `SELECT` verify the deployed database over SSH. No CLI trigger for
a database capture exists yet; PostgreSQL/MySQL server adapters remain
intentionally deferred rather than release blockers.

## Milestone 4 — restore engine (P0)

- Deterministic restore planner, diff, dry-run, confirmations, safety backup,
  staged switch-over, rollback, and signed report.
- Safe extraction rules for paths, links, owners, permissions, limits, and
  special files.
- New-host bootstrap assistant with explicit prerequisites.
- Cross-version format compatibility suite.

Exit gate: automated clean-room restore drill meets documented RTO/RPO and
proves rollback for every supported stack type.

Foundation implemented: a core restore planner accepts only a sealed manifest,
an absolute target path, and supported filesystem payloads. It generates an
exact confirmation phrase but performs no extraction or target mutation. The
local repository adapter creates it only after re-verifying the sealed backup's
signature and payload checksums, and rejects an existing target path.
The "New-host bootstrap assistant" line above is now implemented as a
CLI-only remote deploy adapter (ADR 0007, `guardian-cli deploy plan|execute`):
it pushes a sealed backup's filesystem and (if present) database payload onto
an absent path on a different, separately-enrolled, host-key-pinned profile
over the existing reviewed SSH invocation, guarded by a target-profile/host
self-overwrite check, an exact confirmation phrase, and an atomic-rename
remote command so an interrupted push never leaves a partial target. Each
payload's manifest signature is re-verified immediately before that payload
is pushed. Every attempt is recorded to the repository's audit log. A
desktop Deploy view now previews and executes the same plan against an
enrolled target profile, mirroring the CLI. An automated clean-room drill
now exists (`npm run test:integration:drill`, closing part of this
milestone's own exit gate): it proves that real capture, local restore, and
remote deploy round-trip a filesystem-plus-database backup correctly
against disposable fixtures, and records elapsed time and a
machine-readable report per `OPERATIONS_RUNBOOK.md`. It does not prove
rollback for any stack type, since restore/deploy rollback itself is not
implemented yet (below) — its own report records this explicitly rather
than silently passing over it, so the exit gate remains not met. This is
test infrastructure only: it calls already-shipped, already-reviewed
composition roots (ADR 0004/0005/0006/0007) from a new test and changes no
production behavior, so it did not need a new ADR of its own. Diff/dry-run
file-level preview, a pre-restore/deploy safety backup of an existing
target (today both simply refuse to run against a non-absent target
instead of backing it up first), staged switch-over, restore/deploy
rollback (distinct from retention's own whole-backup-deletion rollback,
which already exists), a signed report, service stop/start orchestration,
and database-aware live cutover remain open.

## Milestone 5 — desktop product and scheduling (P1)

- Profile and plan editor with validation and least-privilege guidance.
- Job timeline, warnings, verification, retention, and restore preview UI.
- Native Windows Task Scheduler and Linux systemd integration using the CLI.
- Local notifications and auditable diagnostic export.
- Accessibility, keyboard, reduced-motion, failure, and cancellation coverage.

Exit gate: non-technical operator can configure, schedule, verify, and drill a
backup without terminal use; UI never bypasses core policies.

Initial desktop slice implemented: the Overview now offers a single SSH server
enrollment form backed by a native key-file picker. It generates opaque
profile and credential references, stores only the public profile locally,
places the selected supported key in the OS credential store, requires
explicit host-key verification acknowledgement, and performs a fixed pinned
SSH connection probe and a read-only `tar --zstd` capability preflight after
enrollment. The Overview also registers a single selected local folder as its
own isolated `LocalRepository` (atomic staging/sealed layout; only the
canonical path, label, and repository ID are stored in the local application
registry), and saves/runs a capture plan end to end against an enrolled
server and registered repository. Separate Restore and Deploy views preview a
plan, require the exact typed confirmation phrase, and execute it — against
the local repository, or a different pinned target profile over SSH,
respectively — mirroring `guardian-cli restore`/`deploy` in the GUI. The
capture-plan form can also list a selected server's Docker containers and
add a capturable bind-mount or named-volume path as a root with one click
(container name/state/mounts only; image, health, Compose project, secret
references, and networks are discovered but not yet surfaced). Enrollment
recovery/credential cleanup, scheduling, and a browsable backup/activity
timeline with warnings and verification state remain open.

## Milestone 6 — release hardening (P1)

- Signed Windows installer and Linux AppImage/deb packages.
- Signed updater metadata, SBOM, provenance, dependency/license audit.
- Performance budgets, long-duration tests, disk exhaustion, network loss, and
  power-loss simulations.
- Independent-node/offline copy workflow and documented 3-2-1 strategy.
- External security review and remediation.

Exit gate: reproducible release from a signed tag, two-platform CI green,
external review complete, and published clean-room restore evidence.

## Later candidates (P2)

- S3-compatible object storage with Object Lock and client-side encryption.
- Repository replication between mutually authenticated nodes.
- Optional malware scanner adapters.
- Kubernetes-aware capture plans.
- Organization policy bundles and approval workflows.

These do not block the initial production release and must not weaken the
independent-backup model.

## Definition of production-ready

The label is allowed only after all P0 milestones, encrypted payloads, signed
artifacts, Windows/Linux CI, hostile-input testing, external security review,
and a documented restore drill succeed. Backup creation without verified
restoration is not completion.
