# CODEX Architecture Guide

This is the architecture contract for VDS Guardian. `AGENTS.md` defines the
operational rules for changing the repository; the documents under `docs/`
define product and security decisions. If code conflicts with those documents,
the change is incomplete until either the code or a reviewed ADR is updated.

## Target product

VDS Guardian is one product with two delivery surfaces:

- a Tauri desktop application for Windows and Linux operators;
- a headless CLI/service for unattended Linux or Windows backup nodes.

Both surfaces call the same Rust use cases. The UI, Tauri commands, and CLI may
orchestrate work but must not implement backup, verification, retention, or
restore rules independently.

## Non-negotiable invariants

- A backup is written to a new staging directory and is immutable after seal.
- A failed, cancelled, unverified, or policy-violating run is never marked
  restorable.
- Restore reads only a supported, verified manifest and defaults to dry-run.
- Database backups use quiesce/dump adapters; live database storage is never
  treated as an ordinary file tree.
- Remote input, manifests, paths, and command output are untrusted and validated.
- SSH host keys are pinned. A changed host identity fails closed.
- Private keys and passphrases never enter repository configuration or logs.
- Destructive server mutations require an explicit plan, scope preview, typed
  confirmation, audit record, and a fresh pre-restore backup unless waived by a
  recorded break-glass decision.
- Backup repositories do not execute hooks or binaries recovered from a server.
- Completed backups are append-only through application APIs. Retention deletes
  whole backup directories only after policy evaluation.

## Layer contract

```text
React UI / CLI parser
        |
Tauri bridge / CLI adapter
        |
application use cases
        |
domain model + ports
        |
SSH, filesystem, keyring, scheduler, archive, database adapters
```

- `guardian-core` owns domain types, state transitions, policies, and use cases.
- Infrastructure is accessed through narrow traits and injected into use cases.
- `#[tauri::command]` functions validate DTOs, invoke one use case, and map a
  typed result. They do not run SSH commands or touch backup storage directly.
- React imports Tauri APIs only through `src/shared/commands.ts`.
- CLI output is stable and scriptable; `--json` is required before CLI commands
  are declared stable.

## Rust rules

- `unsafe` is forbidden unless an ADR documents why it is unavoidable.
- `unwrap`, `expect`, `panic`, `todo`, and `unimplemented` are denied in product
  code. Bootstrap exceptions need a narrow allow and an explanatory comment.
- Domain identifiers use newtypes; filesystem paths and remote paths are never
  interchangeable strings.
- Use typed errors with safe public messages and structured internal context.
- Shell command strings are forbidden. Remote commands are built from reviewed
  command templates with individually escaped/validated arguments.
- Blocking work must not run on the Tauri UI thread.

## TypeScript and UI rules

- TypeScript strict mode is mandatory; avoid `any` and validate native DTOs.
- UI state is presentation state. Backup lifecycle truth comes from core events.
- Long-running jobs use explicit states: queued, running, verifying, sealing,
  succeeded, failed, cancelled, quarantined.
- Destructive buttons cannot be the default focused action and must explain
  target, scope, and rollback posture.
- All controls require accessible names and keyboard operation.
- UI modules should remain under 300 lines and functions under 40 lines unless
  a documented exception is clearer than a split.

## Observability and audit

- Every job has a UUIDv7 correlation ID.
- Audit events are append-only and redact secrets, key paths, raw environment
  values, and sensitive command output.
- Metrics use bounded labels only. Hostnames and backup IDs do not become metric
  labels.
- A user-visible failure has an error code and safe remediation hint; detailed
  diagnostics are opt-in exports.

## Release contract

- Linux and Windows gates pass from the same commit.
- Packages are produced from signed tags and include checksums and an SBOM.
- Desktop updates use signed release artifacts, never `git pull` inside an
  installed application.
- A release that changes backup format, SSH behavior, or restore logic requires
  a clean-room restore drill before publication.

