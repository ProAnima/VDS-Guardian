# Operations Runbook

Status: foundation only. Commands below describe the required operator contract;
live commands will be enabled by milestones in `DEVELOPMENT_PLAN.md`.

## Normal backup

1. Preflight the node, repository, SSH trust, disk space, and remote capabilities.
2. Preview the resolved plan and estimated requirements.
3. Start a job and monitor structured phases.
4. Confirm the result is `sealed`, not merely `captured`.
5. Review warnings and the verification report.

## Programmatic and agent access

`guardian-mcp` (ADR 0012) exposes the same capture/restore/deploy/discovery
operations as MCP tools, for external tools and AI agents rather than a human
at the desktop app or a terminal. It runs as a local subprocess over stdio
only — never a network-reachable transport — so it carries the same
OS-process trust as the desktop app and CLI, not a wider one. Restore and
deploy tool calls require the exact confirmation phrase a prior preview call
returned, identical to the desktop and CLI flows; the calling agent supplies
it explicitly, standing in for the human who would otherwise type or paste
it. Capture, deploy, and cancellation use the same run-id-keyed job registry
the desktop app uses, so a capture or deploy started via MCP can be
cancelled the same cooperative way.

## Scheduled backup

Scheduled jobs must be non-interactive and therefore cannot enroll a new host
key, unlock an unavailable secret, change a backup plan, or accept a warning
that violates policy. Such jobs fail closed and notify through configured local
channels.

## Restore drill

At least one clean-room restore drill is required before a release can claim
production readiness. The drill must start from a fresh target, verify the
backup, execute the exact generated plan, check application health and data,
record RTO/RPO, and preserve a machine-readable report.

An automated version now exists (`npm run test:integration:drill`,
`crates/guardian-capture/tests/clean_room_drill.rs`): it captures a real
filesystem-plus-database backup from a disposable container, restores it to
a fresh local target, and separately deploys it to a second fresh disposable
host — verifying byte-exact filesystem content plus a real `PRAGMA
integrity_check` and row query against the database, both locally after
restore and over SSH after deploy (a SQLite `.backup` is a logical copy
through the database engine, not a raw byte copy, so its own bookkeeping
header fields legitimately differ from the source; only the filesystem
payload is verified byte-exact). It records elapsed time and a
machine-readable JSON report per run. Both `restore_drill` and
`deploy_drill` passed end to end for the first time on 2026-07-16, after
fixing two previously undiscovered defects in archive path validation and
one drill-fixture permission gap (see ADR 0011) that had silently blocked
every earlier attempt. It does not prove rollback for any stack type —
restore/deploy rollback is not implemented — and does not cover every
supported stack type or failure mode, so it does not by itself satisfy the
requirement above for a release claim. Run the drill manually for anything
the automated version does not yet cover, or when CI access is unavailable.

## Incident rules

- Changed SSH fingerprint: stop; verify through an independent channel.
- Checksum/signature failure: quarantine; never repair the sealed original.
- Suspected source compromise: isolate the newest backups, retain prior recovery
  points, and restore only after incident review.
- Low disk: do not delete the newest backup opportunistically; run reviewed
  retention against sealed backups and preserve the configured minimum set.
- Lost signing key: preserve old public keys and sealed backups; enroll a new
  signer for future backups.

