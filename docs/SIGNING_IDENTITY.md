# Backup-Node Signing Identity

Status: Milestone 1 implementation contract. Enrollment is not yet exposed by
the desktop application or CLI.

## Purpose

Every backup node owns an independent Ed25519 identity. The private 32-byte seed
is stored as a binary secret and is never written to a backup repository,
manifest, log, configuration export, or application resource. Manifests contain
only the algorithm and a key ID derived from the SHA-256 digest of the public
verification key.

## Storage mapping

- Service name: `ProAnima.VDSGuardian`.
- Account name: validated random credential ID from local configuration.
- Windows backend: Windows Credential Manager.
- Linux desktop backend: Secret Service over the user session.

A missing, locked, malformed, or unavailable credential store fails closed.
Headless Linux nodes without a usable Secret Service need the separately
reviewed encrypted-vault fallback described in ADR 0002; plaintext seed files
are not an accepted fallback.

## Lifecycle

1. Hold the exclusive node-configuration lock.
2. Reject enrollment if the credential ID already contains a secret.
3. Generate the seed from the operating-system CSPRNG.
4. Store the binary seed, read it back, and compare the public verification key.
5. Release the lock only after the credential reference is atomically committed.

Loading validates the exact 32-byte length. Signing keys and temporary seed
buffers use zeroization on drop. Errors are typed and never include credential
contents or platform error payloads.

Rotation will enroll a new credential ID and preserve old public verification
keys. It must never overwrite manifests or sealed backups. The rotation and
trusted-public-key registry are not implemented in this slice.
