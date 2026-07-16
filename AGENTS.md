# AGENTS.md — VDS Guardian

Mandatory operating contract for every coding agent and contributor working in
this repository. Read this file, `CODEX.md`, and the relevant document under
`docs/` before changing code.

## Current status

Release 0.1 hardening: pinned system-OpenSSH capture seals encrypted
format-v2 filesystem backups with an optional SQLite snapshot into one safe
multi-payload restore/deploy transaction; every payload's data key is also
wrapped under a portable, per-repository recovery key that an operator can
export into a passphrase-protected offline bundle and import on a clean
machine (ADR 0013); capture/deploy cancellation is wired through CLI,
desktop, and `guardian-mcp` adapters; and the desktop app, CLI, and
`guardian-mcp` share one application-service boundary (ADR 0012). The
release is not production-ready: the operator-facing setup/status/restore
flow is incomplete (section 4), and a clean-machine drill must still prove
the compiled production path end to end on Linux CI. The recovery-key import
path is now included in that drill locally (section 5). Docker discovery,
additional databases, scheduling, retention automation, and updater work
are outside Release 0.1. The ordered scope and gates live in
`docs/DEVELOPMENT_PLAN.md`.

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
npm run verify
```

On Windows PowerShell use `npm.cmd`. `verify` is the single canonical gate: it
runs the environment doctor, formatting, lint, strict type checks, frontend
tests/build, Rust Clippy, and Rust tests. Use individual commands only to
diagnose a failed gate. Never claim the whole repository is green after running
only a focused test.

Changes to backup/restore, archive parsing, storage lifecycle, remote command
construction, secret handling, or retention also require security tests and,
where relevant, a run of the clean-room drill (`npm run
test:integration:drill`, Docker-gated). The drill exists and passed
end-to-end for the first time on 2026-07-16 (`docs/adr/0011-archive-path-validation-hardening.md`),
but it is not part of canonical `npm run verify`, has not yet been observed
passing end to end on Linux CI (the latest run stopped on a now-fixed
Unix-only Clippy finding before the drill), and does not cover every failure mode named in
`docs/DEVELOPMENT_PLAN.md` section 5. State explicitly which of these gaps
still apply rather than claiming full production verification.

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
- CLI, GUI, and the MCP server share use cases and serialized DTO contracts
  where appropriate. Capture is deliberately desktop- and MCP-only; the CLI
  does not grow a capture command.
- Infrastructure errors are mapped to typed domain/application errors before
  reaching UI or CLI output.

## Code budgets

- Rust module: target <= 300 lines; function: target <= 40 lines. These are
  split signals, not machine-enforced — no clippy line-count lint exists.
- TypeScript module: <= 300 lines; function: <= 40 lines. These ARE
  machine-enforced today, at ESLint error severity (`max-lines`/
  `max-lines-per-function`, part of canonical `npm run verify`) — not a
  soft target. No `eslint-disable` exception exists anywhere in the
  codebase today.
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
- auto-update, installer, or release signing;
- a new external-facing interface or protocol surface (e.g. a new inbound
  transport, API, or listener) — the trigger that produced ADR 0012.

Stop on a critical doctor or security finding. Do not deploy, restore, publish,
or work around the finding silently.

## Completion language

Distinguish clearly between scaffolded, implemented, locally tested,
integration-tested, restore-drilled, and production-ready. Compilation alone is
not proof that backups can be restored.
