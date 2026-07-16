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

- Windows and Linux desktop application backed by one shared Rust engine, the
  sole first-class human interface;
- a scriptable CLI for enrollment, restore, and deploy (not capture);
- a typed external API (an MCP server) over the same application-service
  boundary, for external tools and AI agents;
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

- Rust core shared by Tauri, CLI, and MCP surfaces;
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
  guarded by a fresh existence check immediately before it. Closed for
  deploy too: a combined deploy now stages both payloads under one shared
  remote directory (neither push renames into place) and publishes with a
  single separate finalize rename; a filesystem-only deploy is unchanged,
  since a single push with an immediate rename was already fully atomic.
- Make attempted/completed/failed audit persistence part of the mutating
  application use case rather than a responsibility duplicated by CLI and
  Tauri callers. Closed: both `DeploymentComposition::execute` and
  `FilesystemCaptureComposition::execute` now write their own audit trail
  unconditionally; every caller (CLI deploy, desktop deploy, desktop
  capture) dropped its own duplicated wrapping.
- Add regression tests for each failure mode above. Covered for all five
  closed items above. Local restore now also has compiled-CLI clean-room proof
  of the exact late-failure sequence: a valid filesystem payload is processed
  first, then a correctly signed and encrypted but semantically invalid
  database payload fails, and no destination is published. Simulating "the
  filesystem push succeeds, the database push then fails" for deploy is still
  not possible at the unit level (the concrete `SystemOpenSsh` a deploy
  composition holds fails every push identically once SSH is unreachable at
  all); a live deploy fault-injection case remains open. The drill's first-ever
  successful run
  (2026-07-16) also surfaced and closed two independent, previously
  undiscovered defects in `guardian-archive`'s path validation — see ADR
  0011 — that had silently rejected real captured directories since this
  validation logic was first written; both `restore_drill` and
  `deploy_drill` now pass end to end for the first time.

Gate: a failed or cancelled restore/deploy leaves no published partial target,
does not delete a path it did not create, and records an accurate terminal
state. Met for both restore and deploy, and for the audit trail on both.

### 2. Make encrypted backups independently recoverable

The OS credential store is useful working storage but cannot be the only copy
of payload keys. Losing the operator machine must not make an intact backup
disk unreadable.

- Decide and document a portable recovery-key model in an ADR that supersedes
  the local-keyring-only consequence of ADR 0004. Closed: ADR 0013.
- Wrap per-backup data keys with a repository recovery key. Closed: every
  payload's data key is additionally wrapped under one repository-wide
  recovery key and the wrapped copy is signed into that payload's own
  manifest entry; live capture fails closed if the target repository has no
  configured recovery key.
- Provide an explicit encrypted recovery bundle and a documented offline-copy
  procedure; ordinary settings export must still contain no plaintext secret.
  Closed at the shared-service/desktop/CLI level: `guardian-local-repository` seals
  the repository recovery key under an Argon2id-derived, passphrase-protected
  bundle bound to the repository id; desktop and `guardian-cli recovery export`
  are adapters, and `docs/OPERATIONS_RUNBOOK.md` documents the offline-copy
  procedure. No settings-export feature exists yet to carry a plaintext secret.
- Prove restore on a clean operator machine that has the repository and recovery
  bundle but no original OS credential-store state. Closed at the CLI/core
  level and exposed through desktop: the bundle authenticates both the recovery key and public manifest
  verification key; `guardian-cli recovery import` installs them for a
  fresh `SecretStore`, proven by
  `init_export_import_recovers_byte_identical_key_material_on_a_fresh_secret_store`,
  which now verifies the sealed manifest and decrypts its real encrypted
  payload without the original signing seed; restore fallback is also proven by
  `restore_falls_back_to_the_recovery_key_when_the_primary_key_is_missing`
  (`crates/guardian-local-repository/tests/restore.rs`). The automated
  clean-room `restore_drill` now also builds the production CLI, removes the
  original vault/signing/registry state, imports the bundle into a new vault
  and registry, and performs the restore through that compiled CLI.
- Keep key import/export explicit, confirmation-gated, and testable from CLI.
  Closed: `recovery export`/`recovery import` both require a typed
  confirmation phrase computed from the repository id, matching the
  restore/deploy confirmation-gate convention; neither is exposed through
  `guardian-mcp`.

  The CLI is the clean-machine/headless adapter, not the owner of this use
  case. Section 4 must move bundle export/import through one shared Rust
  service and expose it from desktop; MCP remains deliberately excluded.

Gate: a clean machine can verify and decrypt an existing backup using only the
documented recovery material, while a missing or incorrect recovery key fails
closed. Met both by the continuous CLI/core test and by the compiled-CLI
clean-room restore drill described above.

### 3. Finish one shared application workflow

- Expose capture, verify, list, restore, deploy, and cancel through one
  application-service boundary shared by every surface. Closed for the
  layer that determines a sealed backup's actual bytes/manifest (`guardian-
  capture`/`guardian-deploy`/`guardian-local-repository`, already shared by
  all three surfaces); each surface's own DTO/directory-resolution glue
  still differs, deliberately, matching this project's own thin-adapter
  discipline rather than one literal shared crate.
- The desktop app is the sole first-class human interface. Headless and
  programmatic access — including AI agents — is served by a typed
  external API (an MCP server), not by a CLI capture command; `guardian-cli`
  keeps its existing enrollment/restore/deploy scope and does not grow one.
  Closed: `guardian-mcp` (ADR 0012, stdio transport only) exposes discovery,
  capture, restore, deploy, and cancel as MCP tools, mirroring the exact
  confirmation-phrase gates CLI and desktop already enforce.
- Keep Tauri commands, CLI parsing, and the API layer thin: each validates
  DTOs, invokes one operation, and maps one typed result. Closed for all
  three surfaces.
- Return bounded progress states and stable error/remediation codes. Tool
  calls stay synchronous (matching CLI/desktop precedent); no progress-
  notification streaming yet.
- Keep filesystem paths explicit; Docker discovery is optional input assistance,
  not part of capture correctness.

Gate: the same fixture plan produces the same sealed backup and restore result
when triggered through the desktop app and the API layer. Met at the
composition-root level (see above); no automated test yet drives the
literal desktop code path and the `guardian-mcp` code path side by side for
byte-identical comparison — `guardian-mcp`'s own tests cover its tool
surface and a real (in-memory transport) MCP protocol round trip
independently.

### 4. Complete the operator path

- The desktop setup flow covers repository, server profile, selected paths, and
  recovery-key readiness without exposing internal architecture. Partially
  closed: desktop exports and imports an offline recovery bundle through the
  shared service; import authenticates the bundle before registering a new
  repository.
- Restore picker exposes only sealed, freshly signature-verified backups and
  labels that verification state; failed and cancelled runs are never offered
  as restore candidates.
- Restore preview states the source backup, destination, expected payload, and
  rollback posture before confirmation. Closed in the UI.
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
  adapters against disposable source and destination hosts. Closed for the
  basic case: the clean-room drill's first-ever successful run (2026-07-16,
  ADR 0011) proved this for a filesystem-plus-SQLite backup, both restored
  locally and deployed to a second disposable host, over a real SSH round
  trip — the first time any test has connected the compiled adapters to a
  live network round trip end to end.
- Import the offline recovery bundle on a clean operator machine before
  restore. Closed locally: `restore_drill` removes the original vault,
  signing configuration, and repository registry, initializes clean state,
  imports through the compiled `guardian-cli`, and executes restore through
  that same binary. Closed on Linux CI too: workflow run `29518019511` passed
  this path for commit `3912a90`.
- Test filesystem-only and filesystem-plus-SQLite backups. Closed: the
  filesystem-only drill captures `/srv/app` over pinned SSH, seals one
  encrypted filesystem payload, restores it through the production repository
  path, and byte-compares the captured configuration; the existing combined
  drill covers filesystem plus SQLite.
- Cover cancellation, corrupted payload, missing recovery key, disk exhaustion,
  changed host key, hostile archive metadata, and failure of the second payload.
  Partially closed: the compiled-CLI restore drill now proves that a corrupted
  encrypted filesystem payload and a missing recovery key both fail without
  publishing the destination. It also proves that a wrong recovery-bundle
  passphrase leaves no repository registration. It also proves local restore's
  late second-payload failure cleanup with a valid filesystem payload followed
  by an invalid SQLite zstd stream. Separate live capture and deploy drills
  wait until the real stream has transferred its first byte, then cancel
  through `JobRegistry`. They prove `cancelled` (not `failed`) audit state;
  capture leaves neither local staging nor a sealed backup, and deploy leaves
  neither target nor remote staging published. A live capture drill also
  connects to a working source with another freshly generated, valid host key
  pinned in its profile; strict host-key checking rejects it before staging and
  sealing. A real SSH preflight plus a deterministic zero-free-space repository
  boundary also proves disk exhaustion fails before staging and sealing, without
  filling an operator disk. A combined deploy drill then succeeds at staging
  the filesystem payload before an invalid database zstd stream fails; it
  proves that the second push removes the entire remote staging tree and never
  publishes the target. Hostile archive metadata is covered at the archive
  boundary but not yet in this live drill.
- Record byte/data integrity, elapsed time, and cleanup state. Closed for the
  basic case: both drill reports record phase timings, an RTO, and per-check
  pass/fail state (`target/drill-reports/*.json`).
- Run the drill from the same commit on Linux CI; keep Windows canonical gates
  green and perform a documented Windows desktop smoke test. Closed for CI:
  workflow run `29518019511` passed both the Ubuntu `verify → SSH integration
  → clean-room drill` chain and the Windows canonical gate for commit
  `3912a90`. A subsequent docs-only run exposed that Rust's default parallel
  test execution could race three live SSH containers on a small runner; the
  drill is now serialized with bounded fixture-readiness deadlines. A
  documented interactive Windows desktop smoke test remains open.

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
