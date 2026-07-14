# Backup Format Contract

Status: Milestone 1 draft. The local repository slice implements the directory
boundary, validated payload paths, SHA-256 verification, Ed25519 signature
metadata, quarantine, atomic seal, and a byte-exact format-v1 golden fixture.
The fixture now prevents silent serialization drift. A fixture corpus also pins
the fail-closed archive-entry path contract. The `guardian-archive` adapter now
inspects tar.zst streams before any extraction: it accepts only regular files
and directories, validates every path, and applies entry and byte limits. Full
plan/item schemas, archive writer and extraction behavior, key rotation
fixtures, and restore compatibility evidence are still required before this
contract is declared stable.

## Directory layout

```text
repository/
  repository.json
  staging/
    <run-id>/
  backups/
    2026-07-13T120000Z_<backup-id>/
      manifest.json
      manifest.sig
      payload/
        filesystem-000.tar.zst
        postgres-main.dump.zst
        docker-metadata.json.zst
      reports/
        capture.json
        verification.json
  quarantine/
    retention-<plan-id>/
  audit/
    retention-<plan-id>-approved.json
    retention-<plan-id>-completed.json
```

Every backup directory is self-describing and independent. No payload depends on
blocks stored only in another backup. Deduplication may be added only as an
explicit repository mode because cross-backup chunk sharing weakens physical
independence and complicates deletion and recovery.

## Manifest minimum fields

- `formatVersion`
- `backupId`, `runId`, `createdAt`, `sealedAt`
- producer name/version/platform
- source profile ID and pinned host-key fingerprint (not hostname secrets)
- plan ID/version/digest
- consistency level and quiesce results
- payload entries: logical role, relative path, byte length, SHA-256, media type
- required/optional item results
- encryption/signature metadata identifiers
- warnings and verification state

The manifest uses canonical JSON for signatures. Secret values, raw environment
files, and private key paths never appear in metadata. Payload encryption is a
planned requirement before production release; the exact envelope receives its
own ADR after a cryptographic review.

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
