# Backup Format Contract

Status: draft for Iteration 0. The first implementation must add golden fixtures
before this contract is declared stable.

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
  audit/
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

