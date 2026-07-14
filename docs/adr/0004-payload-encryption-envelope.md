# ADR 0004: Per-payload AES-256-GCM envelope

## Status

Accepted.

## Context

Sealed payloads currently have integrity and signature protection but are not
confidential at rest. A production repository must not place encryption keys in
the manifest, repository configuration, or backup directory.

## Decision

- Each payload receives a fresh random 256-bit data key and 96-bit nonce.
- Payload bytes are encrypted with AES-256-GCM before registration and sealing.
- The data key is stored only in the operating-system credential store under a
  random credential reference; the manifest contains only a public envelope
  version, algorithm identifier, nonce, and credential reference ID.
- The payload path uses the `.enc` suffix and its manifest digest covers the
  ciphertext exactly as stored.
- Encryption/decryption authenticates immutable associated data: backup ID,
  payload path, and envelope version.
- A missing key, invalid tag, unsupported envelope version, or mismatched
  associated data fails closed. No plaintext is written to a final destination
  until authentication succeeds.

## Consequences

The local keyring becomes a required restore dependency for encrypted backups.
Cross-node recovery and portable key export need a separate, explicit wrapped
key/recovery design. Existing unencrypted format-v1 backups remain readable only
through an explicit compatibility path and cannot silently claim encryption.
