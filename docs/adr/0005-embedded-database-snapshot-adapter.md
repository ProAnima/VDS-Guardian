# ADR 0005: Embedded-database snapshot adapter using `sqlite3 .backup` + zstd

## Status

Accepted.

## Context

Milestone 3 requires an application-consistent snapshot adapter for a
lightweight embedded database (SQLite or an equivalent application-owned
file) as an initial-product capability; PostgreSQL/MySQL server dump/restore
are explicitly out of scope for the first release (ADR 0003 already covers
the version-parity rule for those, once built). Naively copying a live
SQLite file is unsafe: a database under write load may be captured mid-write,
and WAL-mode databases keep uncommitted state in separate `-wal`/`-shm`
sidecar files that a plain file copy would not (or would inconsistently)
include.

## Decision

- Capture uses the fixed, reviewed remote command:

  ```sh
  [ -f '<path>' ] || exit 1; tmp=$(mktemp) || exit 1; sqlite3 '<path>' ".backup '$tmp'" && zstd -q -c "$tmp"; status=$?; rm -f "$tmp"; exit $status
  ```

  `<path>` is the one already-validated, operator-configured absolute file
  path, `shell_quote`'d exactly like every other remote command in
  `guardian-ssh`. `sqlite3 .backup` uses SQLite's Online Backup API, which
  produces a fully consistent copy of a live database — including WAL
  content — without pausing the application. `[ -f ... ] || exit 1` fails
  closed if the configured path does not exist as a regular file, instead of
  letting `sqlite3` silently create and "successfully" back up a fresh empty
  database at a mistyped path. The remote temporary file is always removed
  before the command exits, on both the success and failure paths.
- Unlike PostgreSQL/MySQL (ADR 0003), no server/tool version-parity gate is
  needed: SQLite has no client/server split, so there is no wire or dump
  format compatibility question to resolve. A narrow `probe_sqlite3`
  capability check (mirroring the existing `probe_tar_zstd` boolean
  preflight) confirms the tool is present before a capture is attempted.
  `zstd` presence is already proven by the existing tar-capture preflight.
- The payload is not a tar archive; it is one zstd-compressed file. Capture
  registers it as `logical_role: "database"`,
  `media_type: "application/vnd.sqlite3+zstd"` — new conventions, not
  enforced by type (as `"filesystem"` / `"application/zstd"` already are
  not). It goes through the same mandatory payload encryption as the
  filesystem payload. A new `ArchiveInspectionPort` implementation
  (`ZstdFileInspector`) validates the captured stream is a well-formed,
  bounded zstd stream, mirroring `TarZstdInspector`'s role for the tar path.
- `RestorePlan` gains a second, independent, optional
  `database_payload: Option<PayloadPath>` field (found by `logical_role`,
  not `media_type`) alongside the existing, unchanged, required
  `filesystem_payload`. This is additive: a sealed backup without a database
  payload simply restores as it always has. Restoring the database payload
  decrypts it (reusing the existing encryption envelope unchanged) and
  zstd-decompresses it directly to `<destination>/database.sqlite` — no tar
  unpacking, since there is nothing to unpack.
- The embedded-database capture composition (`guardian-capture`) produces
  its own independently sealed backup today; it is not yet combined with the
  filesystem capture composition into one unified multi-payload plan. That
  integration, along with CLI/desktop UI to trigger a database capture, is
  deferred to a follow-up change once a real multi-item capture plan exists.

## Consequences

- A real, restorable embedded-database capability now exists end to end
  (capture → seal → restore), closing the largest concrete gap against
  Milestone 3's exit gate.
- The operator's backup account needs read access to the configured
  database file and a working `sqlite3` binary on the remote host; neither
  is verified beyond the fixed capability probe and the `[ -f ... ]` guard.
- Non-SQLite embedded engines, PostgreSQL/MySQL dump/restore, and a unified
  multi-payload backup plan remain future work and are not weakened or
  precluded by this design.

## Amendment (2026-07-15): unified into the filesystem capture plan, desktop UI added

The "follow-up change" this ADR deferred above has landed, resolving it
directly rather than leaving the database adapter unreachable from any
surface. Before wiring a "run" action to *any* surface, research surfaced a
blocker with the obvious approach (a standalone `DatabaseCapturePlan`, saved
and run independently): a database-only sealed backup (one payload,
`logical_role: "database"`, zero `application/zstd` filesystem payload)
cannot be restored or deployed by this codebase. Both `RestorePlan::build`
and `DeploymentPlan::build` (ADR 0007) call a shared
`guardian-core::manifest::select_payloads` that hard-requires exactly one
filesystem payload — proven by the existing
`deploy_plan_rejects_a_backup_with_no_filesystem_payload` test. Shipping a
"run snapshot" button producing a validly-signed, permanently unrestorable
backup would be exactly the gap `docs/DEVELOPMENT_PLAN.md`'s own "Definition
of production-ready" section exists to prevent.

Decision: `FilesystemCapturePlan` (`guardian-core/src/plan.rs`) gains an
optional `database_path: Option<String>` (last field, append-only,
`skip_serializing_if` none, so an existing plan's canonical hash is
unaffected). Running a plan with a database path captures the filesystem
root(s) *and* the database file into **one** sealed backup with both
payloads — a shape restore/deploy already fully support today, so neither
needed any change. Rejected alternative: keep the database capture
standalone and instead relax `select_payloads`/`RestorePlan`/`DeploymentPlan`
to accept a database-only manifest (making `filesystem_payload` optional,
symmetric with the already-optional `database_payload`). That would touch
the two most security-reviewed flows in the project for the benefit of a
narrower use case; unifying needed zero changes there. Accepted tradeoff: an
operator can no longer take a lightweight database-only snapshot on its own
cadence between full filesystem backups through this flow. The standalone
`EmbeddedDatabaseCaptureComposition`/`EmbeddedDatabaseBackupUseCase` path
this ADR originally introduced is unchanged and still available for
potential future direct use — it is simply not what the desktop UI calls.

Mechanically: `BackupStoragePort::begin` fails if called twice for the same
run, so capturing two payloads into one staging run before one seal needed a
"begin-less" entry point — `EmbeddedDatabaseCaptureUseCase::execute` was
split into itself (begin, then delegate) plus a new
`execute_within_staging` (no begin), used only when a database payload is
the second of two captured together.
`FilesystemCaptureComposition::execute` (`guardian-capture`) gained an
`Option<EmbeddedDatabaseCaptureRequest>` parameter: when absent, behavior is
byte-for-byte identical to before (a separate code path, not a conditional
inside the same function); when present, it captures both payloads into the
same staging run, adds both entries to one manifest, and seals once, with
composition-level fail-closed handling mirroring the existing use-case-level
pattern.

The desktop's existing capture-plan save/run commands and `CapturePlanPanel`
now expose one additional optional field ("Файл базы данных на сервере") —
no new Tauri commands, no new panel, since the plan itself is what unified.
No CLI trigger was added (symmetric with today: filesystem capture-plan
execution is desktop-only, no `guardian-cli` capture/plan/job command exists
for either kind).
