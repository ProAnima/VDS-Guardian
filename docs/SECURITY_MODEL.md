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
bytes. Enrollment still requires caller-held node locking and is not wired to a
live command. Read-only hardening, retention, archive limits, key rotation, and
clean-room restore drills remain mandatory before production use.

### Hostile backup content

- Verification hashes bytes without executing or previewing them.
- Archive readers reject absolute paths, `..`, Windows drive/UNC paths, device
  nodes, unexpected hardlinks, and links escaping the restore root.
- Extraction applies file-count, total-size, per-file-size, depth, and expansion
  ratio limits.
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
