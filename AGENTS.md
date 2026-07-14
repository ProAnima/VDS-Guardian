# AGENTS.md — VDS Guardian

Mandatory operating contract for every coding agent and contributor working in
this repository. Read this file, `CODEX.md`, and the relevant document under
`docs/` before changing code.

## Current status

Milestone 1: domain and local repository. Simulated-source repository and
signing slices implement isolated staging, SHA-256 verification, golden manifest
fixtures, Ed25519 node identities, OS credential-store integration, quarantine,
atomic seal, journaled signing enrollment, and verified whole-directory
retention. Live SSH backup, the desktop enrollment UI, automated power-loss
reconciliation, and restore are not implemented and must not be represented as
production-ready. Signing status/enrollment bridge commands now exist, but no
desktop screen invokes enrollment automatically or implicitly.

## Source of truth

- `CODEX.md`: architecture and non-negotiable invariants.
- `docs/ARCHITECTURE.md`: component boundaries and runtime topology.
- `docs/SECURITY_MODEL.md`: threat model and security controls.
- `docs/BACKUP_FORMAT.md`: on-disk compatibility contract.
- `docs/DEVELOPMENT_PLAN.md`: ordered milestones and acceptance gates.
- ADRs under `docs/adr/`: durable decisions and rejected alternatives.

If a change alters one of these decisions, add or supersede an ADR in the same
change. Do not silently let implementation drift from the documents.

## Canonical checks

Before declaring any non-trivial change complete, run:

```text
npm run doctor
npm run verify
```

On Windows PowerShell use `npm.cmd`. `verify` is the central gate and includes
formatting, lint, strict type checks, frontend tests/build, Rust formatting,
Clippy, and Rust tests. Use individual commands only to diagnose a failed gate.
Never claim the whole repository is green after running only a focused test.

Changes to backup/restore, archive parsing, storage lifecycle, remote command
construction, secret handling, or retention also require security tests and the
restore-drill profile once those gates land. Until they exist, explicitly state
that production verification is incomplete.

## Required engineering behavior

- Preserve user changes and unrelated dirty worktree files.
- Keep commits scoped; never add runtime data, real server addresses, keys,
  backup archives, diagnostics, or generated packages.
- Prefer port/adapter implementations over platform branches in use cases.
- Validate all network, process, manifest, and filesystem boundaries.
- Add tests with behavior. A fix without a regression test needs an explicit
  reason.
- Use atomic writes for mutable local metadata. Never edit a sealed backup.
- Never create shell commands through interpolation. Prefer direct argv locally;
  remote command templates must use reviewed argument encoders.
- Never weaken host-key checks, TLS checks, permission prompts, or checksum
  validation to make a test pass.
- A destructive operation must remain dry-run-first and approval-gated.
- No telemetry leaves the machine unless an explicit future opt-in ADR allows it.

## Boundaries enforced by review and gates

- React components do not import Tauri APIs; only `src/shared/commands.ts` may.
- Tauri commands do not contain business logic or infrastructure orchestration.
- `guardian-core` does not depend on Tauri, React, OS-specific UI, or a concrete
  SSH/keyring/archive implementation.
- CLI and GUI share use cases and serialized DTO contracts where appropriate.
- Infrastructure errors are mapped to typed domain/application errors before
  reaching UI or CLI output.

## Code budgets

- Rust/TypeScript module: target <= 300 lines.
- Function: target <= 40 lines.
- Tauri command: target <= 20 lines.
- New dependencies require a short justification in the PR/change summary,
  including security and cross-platform impact.

These are split signals rather than reasons to obscure cohesive tables or
schema declarations. Exceptions must be documented next to the gate.

## Security review triggers

Update `docs/SECURITY_MODEL.md` and add adversarial tests when changing:

- secret/key storage or authentication;
- SSH trust, privileges, command execution, or remote scripts;
- archive extraction, symlinks, hardlinks, ownership, or paths;
- backup sealing, verification, signing, quarantine, or deletion;
- restore planning or remote mutation;
- auto-update, installer, or release signing.

Stop on a critical doctor or security finding. Do not deploy, restore, publish,
or work around the finding silently.

## Completion language

Distinguish clearly between scaffolded, implemented, locally tested,
integration-tested, restore-drilled, and production-ready. Compilation alone is
not proof that backups can be restored.
