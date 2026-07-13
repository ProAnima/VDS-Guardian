# Operations Runbook

Status: foundation only. Commands below describe the required operator contract;
live commands will be enabled by milestones in `DEVELOPMENT_PLAN.md`.

## Normal backup

1. Preflight the node, repository, SSH trust, disk space, and remote capabilities.
2. Preview the resolved plan and estimated requirements.
3. Start a job and monitor structured phases.
4. Confirm the result is `sealed`, not merely `captured`.
5. Review warnings and the verification report.

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

## Incident rules

- Changed SSH fingerprint: stop; verify through an independent channel.
- Checksum/signature failure: quarantine; never repair the sealed original.
- Suspected source compromise: isolate the newest backups, retain prior recovery
  points, and restore only after incident review.
- Low disk: do not delete the newest backup opportunistically; run reviewed
  retention against sealed backups and preserve the configured minimum set.
- Lost signing key: preserve old public keys and sealed backups; enroll a new
  signer for future backups.

