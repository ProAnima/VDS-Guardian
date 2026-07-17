# Backup Format Contract

Status: draft, reflecting Milestone 1-5 work; not yet declared stable (see
the closing paragraph of this section for what's still required). The local
repository slice implements the directory
boundary, validated payload paths, SHA-256 verification, Ed25519 signature
metadata, quarantine, atomic seal, and a byte-exact format-v1 golden fixture.
The fixture now prevents silent serialization drift. A fixture corpus also pins
the fail-closed archive-entry path contract. The `guardian-archive` adapter now
inspects tar.zst streams before extraction and can extract only into a newly
created destination: it accepts only regular files and directories, validates
every path, applies entry and byte limits, and removes a partial destination on
failure. It also emits deterministic tar.zst headers for validated paths;
verified filesystem restores extract only to a new target directory. New live
filesystem captures use format-v2 encrypted payloads: every `.enc` payload has
a fresh data key held only in the OS credential store, while the manifest
records only public envelope metadata. Format-v1 unencrypted backups remain
readable as an explicit compatibility case. A lightweight embedded-database
(SQLite) snapshot payload (ADR 0005) is now a second supported payload kind:
`logicalRole: "database"`, `mediaType: "application/vnd.sqlite3+zstd"`, a
single zstd-compressed file rather than a tar archive, encrypted the same way
as the filesystem payload. A database payload can be sealed either as its own
independent backup, or combined with a filesystem payload into one sealed
backup from a single capture plan — the desktop's capture-plan flow now
offers an optional database path alongside the filesystem roots and captures
both into one manifest when set. Restore already treats both shapes
identically, since the database payload was always optional and found by
`logicalRole`, not position. Full plan/item schemas, key rotation fixtures,
and restore compatibility evidence are still required before this contract
is declared stable.

## Directory layout

```text
repository/
  repository.json
  staging/
    <run-id>/
  backups/
    <backup-id>/
      manifest.json
      manifest.sig
      payload/
        filesystem-000.tar.zst.enc
        database-000.sqlite.zst.enc
      reports/
        verification.json
  quarantine/
    retention-<plan-id>/
    <run-id>/
  audit/
    retention-<plan-id>-approved.json
    retention-<plan-id>-completed.json
    capture-<run-id>-<state>.json
    deploy-<run-id>-<state>.json
```

The directory name under `backups/` is the backup ID alone, with no
timestamp prefix. Payload filenames carry the `.enc` suffix for the current
format-v2 encrypted case; format-v1 unencrypted backups (still readable as
an explicit compatibility case) omit it. The database payload shown is the
real embedded-SQLite snapshot (ADR 0005) — today's only implemented second
payload kind; PostgreSQL/MySQL dump/restore remains out of scope for the
first release (Milestone 3), so no such payload is ever produced. No
Docker-metadata payload is ever written either — Docker inventory is
discovered live for display and mount selection, never persisted into a
backup. `reports/` only ever contains `verification.json`; there is no
`capture.json`. `quarantine/` also receives run-ID-keyed entries for
abandoned or failed captures, alongside plan-ID-keyed retention entries;
`audit/` also receives per-run capture and deploy attempt records
(`write_capture_audit`/`write_deploy_audit`), alongside the retention
records shown above.

Every backup directory is self-describing and independent. No payload depends on
blocks stored only in another backup. Deduplication may be added only as an
explicit repository mode because cross-backup chunk sharing weakens physical
independence and complicates deletion and recovery.

## Manifest minimum fields

- `formatVersion`
- `backupId`, `runId`, `createdAt`, `sealedAt`
- producer name/version/platform
- source profile ID and pinned host-key fingerprint (not hostname secrets) —
  a frozen record of what was captured; unrelated to which profile a later
  remote deploy targets (ADR 0007), which is supplied at deploy time and
  explicitly checked against these two fields to block self-overwrite
- plan ID/version/digest
- optional signed logical-selection metadata mapping operator-visible filesystem
  or Docker labels to the normalized capture roots; labels are explanatory only
  and never override payload paths, hashes, or restore validation
- consistency level and quiesce results
- payload entries: logical role, relative path, byte length, SHA-256, media type
- required/optional item results
- encryption/signature metadata identifiers; a format-v2 payload includes
  envelope version, algorithm, credential reference, and base nonce
- an optional recovery-wrapped copy of the payload's data key (ADR 0013),
  present whenever the repository had a configured recovery key at capture
  time; absent on backups sealed before that key existed or without one
  configured, which remain restorable only through the primary credential
  reference
- warnings and verification state

The manifest uses canonical JSON for signatures. Secret values, raw environment
files, and private key paths never appear in metadata. Payload encryption is
implemented (format-v2, ADR 0004, AES-256-GCM-CHUNKED) and mandatory for every
live capture, as described above; only pre-existing format-v1 backups are
read without it, as an explicit compatibility case, never a live-capture
option.

## Sealing rules

A backup is restorable only when:

1. all required items succeeded;
2. every payload length and digest matches;
3. the manifest validates against its exact schema version;
4. consistency policy is satisfied;
5. the signature is present and verifies;
6. the staging directory was atomically moved to `backups/`;
7. a verification report records success.

Any ambiguity fails closed. Warning-only backups need an explicit plan policy
and are visibly distinct from fully consistent backups.

Retention never edits a sealed directory. See `RETENTION.md` for the verified
inventory, snapshot approval, and whole-directory deletion contract.
