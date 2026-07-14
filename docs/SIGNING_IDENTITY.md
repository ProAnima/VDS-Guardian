# Backup-Node Signing Identity

Status: Milestone 1 implementation contract. Locked, journaled enrollment is
implemented as a shared Rust service and exposed through explicit JSON CLI and
Tauri bridge commands. The desktop setup screen shows status and offers a
deliberate, acknowledged enrollment flow.

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

1. Acquire the cross-process `signing.lock` in the node configuration directory.
2. Atomically create `signing-enrollment.json` with a random credential ID.
3. Generate the seed from the operating-system CSPRNG only when that credential
   ID does not already contain a secret.
4. Store the binary seed, read it back, and compare the public verification key.
5. Atomically commit `signing.json` with only format version, credential ID,
   algorithm, and derived public key ID.
6. Remove the enrollment intent and release the lock.

If execution stops after the keyring write but before configuration commit, the
intent remains. The next run loads the existing secret and commits its reference
with disposition `recovered`; it never generates a replacement key. A committed
configuration with a missing secret fails closed and never rotates implicitly.
Configuration/key-ID disagreement, unknown fields, unsafe filesystem entries,
and concurrent enrollment also fail closed.

## Application entrypoints

`signing status` is read-only: it reports `not_enrolled`,
`enrollment_pending`, `recovery_pending`, or `ready`. It can verify an existing
credential but never generates a seed, writes a secret, creates an enrollment
intent, or repairs state.

`signing enroll` is the only entrypoint that may start or finish enrollment.
The CLI requires `--json` and an explicit absolute `--config-dir`; malformed or
relative paths are rejected before touching the keyring. Tauri resolves the
application configuration directory and runs keyring/filesystem work on a
blocking worker, never the UI thread. Both surfaces serialize the same DTOs and
safe error codes. Internal paths and platform error payloads are not returned.

The desktop screen first reads status and never enrolls on mount or refresh. A
user must choose enrollment, acknowledge the confirmation statement, and then
choose the final create action. Browser previews cannot invoke enrollment. On
success, the screen reloads and displays only public credential metadata.

Loading validates the exact 32-byte length. Signing keys and temporary seed
buffers use zeroization on drop. Errors are typed and never include credential
contents or platform error payloads.

Rotation will enroll a new credential ID and preserve old public verification
keys. It must never overwrite manifests or sealed backups. Explicit rotation,
trusted-public-key registry, and encrypted-vault fallback are not implemented
in this slice.
