# Security Model

## Assumptions

- The remote VDS may be compromised, malicious, or partially unavailable.
- Backup content may contain malware, hostile filenames, symlinks, devices,
  decompression bombs, and secrets.
- The operator machine and backup repository are trusted at installation time,
  but ransomware or credential theft remain possible.
- SSH provides transport security only after host identity is pinned correctly.
- A successful backup is not proof that the captured application is healthy.

## Primary assets

- SSH private keys and passphrases.
- Backup payloads and manifests.
- Backup-node signing identity.
- Server profiles and pinned host keys.
- Audit records and restore approvals.
- Availability of at least one clean, independently stored recovery point.

## Trust boundaries

1. React WebView to Tauri bridge.
2. Application core to OS/process/SSH adapters.
3. Backup node to remote VDS.
4. Staging directory to sealed repository.
5. Sealed repository to restore target.
6. Installed binary to update/release infrastructure.

## Mandatory controls

### Credentials

- Store secrets in OS credential storage, referenced by random credential ID.
- Never embed a private key in the repository, application resources, logs, or
  portable configuration exports.
- Support operator-selected key files only by reference and validate restrictive
  permissions where the platform supports them.
- Prefer a dedicated backup account with least privilege and reviewed `sudo`
  commands over unrestricted root login.

### SSH

- First connection shows the fingerprint and requires explicit trust.
- Later fingerprint changes fail closed and require a separate re-enrollment
  workflow; no accept-new fallback in scheduled jobs.
- Use timeouts, keepalive, cancellation, output caps, and strict argument
  encoding. Do not upload and execute unversioned shell text from the UI.
- Capability discovery is read-only and becomes part of the backup plan.

### Repository isolation

- Each run writes to `<repository>/staging/<run-id>` on the same filesystem as
  the final location, then seals by atomic rename to `<repository>/backups/<id>`.
- Normal APIs never open a sealed backup for write.
- Retention removes an entire backup directory; it does not rewrite survivors.
- A manifest lists every file, length, digest, media type, and logical role.
- Read-only flags are defense in depth, not an immutability guarantee. Strong
  ransomware resistance requires a second node, offline/removable media, or an
  object store with retention lock in a later milestone.

Milestone 1 currently enforces validated identifiers during deserialization,
slash-only relative payload paths, symlink rejection at write and verification
boundaries, a cross-process writer lock held for the staging lifetime, streaming
SHA-256 verification, Ed25519-only signing metadata, quarantine on seal failure,
and same-filesystem atomic rename. Ed25519 seeds are zeroized in memory and can
be persisted as binary secrets in Windows Credential Manager or Linux Secret
Service under a random credential ID. Golden fixtures pin canonical manifest
bytes. Enrollment orchestration now holds a cross-process configuration lock,
commits a credential reference atomically, and recovers the same key from a
durable non-secret intent after interruption. It is exposed through explicit
CLI and desktop commands, never implicitly.
Retention verifies canonical manifest bytes, Ed25519
signatures, and the exact payload tree before planning or executing a
snapshot-bound whole-directory deletion. Retention deletion now writes a
durable non-secret intent outside its temporary quarantine directory. On the
next repository open, a move-phase interruption is rolled back; a durable
cleanup-ready phase is resumed idempotently. Orphaned or malformed retention
state fails closed. Read-only hardening, archive limits, key rotation, and
clean-room restore drills remain mandatory before production use.

Signing configuration tampering cannot silently select a replacement identity:
the configured public key ID must match the key loaded through its credential
reference. A missing committed secret, incompatible schema, unsafe metadata
file, or concurrent enrollment fails closed. The recovery journal contains only
a random credential ID and format version.

Status inspection cannot initiate enrollment. CLI enrollment requires an exact
verb, JSON mode, and an absolute configuration path. Tauri performs credential
work outside the UI thread, and both adapters return bounded error codes and
remediation text rather than internal paths or operating-system error payloads.
Process-local lock registries close Windows same-process re-entry while OS file
locks continue to serialize independent processes.

The desktop setup screen follows the same boundary: rendering, status refresh,
and browser preview cannot create an identity. Enrollment requires a separate
user action, an acknowledgement checkbox, and a final confirmation; cancelling
clears that acknowledgement.

### Hostile backup content

- Archive entry names use a dedicated cross-platform relative-path type that
  rejects absolute paths, traversal, empty segments, Windows separators/drive
  syntax, alternate streams, and NUL bytes before extraction exists.
- Verification hashes bytes without executing or previewing them.
- The current streaming tar.zst inspector rejects unsafe paths and every entry
  type except regular files and directories, including links, device nodes, and
  other special files. It enforces entry-count, declared per-file, and expanded
  stream-byte limits before extraction exists.
- A future extractor will add depth and expansion-ratio limits, safe ownership
  and permission handling, and destination-root containment checks.
- Restores never preserve setuid/setgid bits by default and use an explicit
  ownership mapping policy.
- Optional antivirus integration is an adapter with timeout and clear
  `not-scanned` versus `clean` states. No scanner result upgrades trust by itself.

### Restore safety

- Default to dry-run and a new destination.
- Verify backup signature/checksums immediately before mutation.
- Re-confirm server identity and show all deletions/service impacts.
- Create a safety point before destructive in-place restore.
- Database restore targets a new database/container first where practical.
- Hooks captured from the server are data, never automatically executable.

### UI and local API

- Tauri capabilities are allowlist-based. No generic shell or filesystem plugin
  is exposed to the WebView.
- Command DTOs are validated, job IDs are unguessable, and errors are redacted.
- Rich backup content is never rendered as raw HTML in the WebView.

## Key rotation

Server credentials and backup-node signing keys have independent identities.
Credential rotation does not rewrite backups. Signing-key rotation creates a
new trusted key record while retaining old public verification keys. Private
signing material is never exported with ordinary settings.

## Residual risks

- A fully compromised backup node can steal keys and alter new backups.
- A compromised privileged VDS account can present internally consistent but
  malicious data.
- Local read-only flags do not stop an administrator or ransomware with equal
  privileges.
- Backing up encrypted application data without its external keys may produce an
  unrecoverable but checksum-valid snapshot.
- Docker image tags are mutable; recovery plans should record digests and retain
  Compose/env material securely.

These risks are addressed operationally through independent nodes, least
privilege, offline/off-site copies, signed releases, and regular clean-room
restore drills.
