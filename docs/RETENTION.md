# Retention Safety Contract

Status: Milestone 1 implementation contract. Retention is available as a core
repository API; desktop and CLI orchestration are not enabled yet.

## Trusted inventory

Retention treats directory names and manifest timestamps as hostile input. A
backup enters the retention inventory only after all of these checks pass:

1. the backup entry and payload root are real directories, never symlinks;
2. the directory name is a valid backup ID and equals `manifest.backupId`;
3. `manifest.json` is the byte-exact canonical sealed manifest;
4. `manifest.sig` matches its algorithm and key ID and verifies through the
   configured trusted-key verifier;
5. the payload tree contains exactly the listed regular files, with matching
   lengths and streaming SHA-256 digests.

One invalid backup stops the entire plan. Retention never selects deletion
candidates from partially trusted metadata.

## Dry run and approval

The dry run sorts trusted backups by canonical UTC `sealedAt`, then backup ID as
a stable tie-breaker. It deletes only the oldest directories above `maxBackups`
and never produces a destructive result below `minimumBackups`.

The plan ID is a SHA-256 digest of the repository ID, policy, complete ordered
snapshot, and deletion set. Execution holds the repository writer lock,
rebuilds the trusted inventory, and rejects any stale or cross-repository plan.
A destructive plan requires the exact generated confirmation phrase. A no-op
plan needs no destructive confirmation.

## Whole-directory deletion

Before mutation, the repository writes a create-only `approved` audit record.
Each selected sealed directory is then atomically moved, without opening it for
write, into `quarantine/retention-<plan-id>/`. A move failure rolls back prior
moves in reverse order. Survivors are never rewritten.

The repository also writes a create-only non-secret intent beside the quarantine
directory. Before cleanup, it creates a separate `cleanup-ready` marker. On
repository open, an intent without that marker rolls moved backups back into the
active inventory; an intent with the marker resumes deletion of only the
approved backup IDs, then writes the create-only `completed` audit record.
Malformed, orphaned, duplicate, symlinked, or contradictory transaction state
fails closed with `RecoveryRequired`.

If cleanup is denied, the API returns `CleanupPending`: the selected backups
are no longer in the active inventory, but their isolated quarantine bytes may
still consume space. The next repository open resumes this same approved cleanup
idempotently; no new retention plan or confirmation is inferred.
