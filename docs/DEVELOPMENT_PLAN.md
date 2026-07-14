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
records append-only audit evidence. Full schemas, archive hostility tests,
and the restore-drill gate remain open. Retention now records a durable,
non-secret transaction intent: reopening rolls back a partially moved deletion
set, or resumes only a cleanup phase that was durably approved.
Signing identity enrollment now uses a
cross-process lock, an atomic public configuration, and a non-secret recovery
intent. Read-only status and explicit enrollment are available through JSON CLI
and Tauri bridge commands; the desktop setup UI requires a separate explicit
acknowledgement before enrollment. Explicit rotation remains open.

## Milestone 2 — secure SSH capture (P0)

- OS keyring credential references and dedicated-key enrollment.
- Known-host pinning, changed-key failure, timeout/cancel/output limits.
- Read-only capability discovery and reviewed remote command templates.
- Streaming filesystem capture with deterministic metadata normalization.
- Integration tests against ephemeral SSH containers and a compromised-server
  fixture suite.

Exit gate: capture from a disposable Linux host is repeatable and cannot escape
the plan or repository under adversarial filenames/output.

## Milestone 3 — Docker and database consistency (P0)

- Docker/Compose inventory: files, project labels, image digests, networks,
  mounts, secrets references, and health state.
- Named-volume and bind-mount capture strategies.
- PostgreSQL and MySQL dump/restore adapters with version compatibility checks.
- Quiesce hooks are shipped, versioned application adapters; arbitrary remote
  scripts are never executed from captured content.
- Encrypted payload envelope ADR and implementation.

Exit gate: fixture stacks with databases restore to a fresh host and pass data
integrity and health checks.

## Milestone 4 — restore engine (P0)

- Deterministic restore planner, diff, dry-run, confirmations, safety backup,
  staged switch-over, rollback, and signed report.
- Safe extraction rules for paths, links, owners, permissions, limits, and
  special files.
- New-host bootstrap assistant with explicit prerequisites.
- Cross-version format compatibility suite.

Exit gate: automated clean-room restore drill meets documented RTO/RPO and
proves rollback for every supported stack type.

## Milestone 5 — desktop product and scheduling (P1)

- Profile and plan editor with validation and least-privilege guidance.
- Job timeline, warnings, verification, retention, and restore preview UI.
- Native Windows Task Scheduler and Linux systemd integration using the CLI.
- Local notifications and auditable diagnostic export.
- Accessibility, keyboard, reduced-motion, failure, and cancellation coverage.

Exit gate: non-technical operator can configure, schedule, verify, and drill a
backup without terminal use; UI never bypasses core policies.

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
