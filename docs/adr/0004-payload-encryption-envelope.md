# ADR 0004: Per-payload streaming AES-256-GCM envelope

## Status

Accepted.

## Context

Sealed payloads need confidentiality at rest without forcing multi-gigabyte
archives into RAM. Keys must not appear in manifests, repository configuration,
or backup directories.

## Decision

- Each payload receives a fresh random 256-bit data key and 96-bit base nonce.
- Payload bytes are encrypted before registration and sealing with
  `AES-256-GCM-CHUNKED`: framed chunks of at most 1 MiB plus an authenticated
  empty final frame. This prevents a valid encrypted prefix from being treated
  as a complete payload.
- The data key is stored only in the operating-system credential store under a
  random credential reference. The manifest contains only envelope version,
  algorithm, nonce, and credential reference ID.
- The payload path ends in `.enc`; the manifest digest covers the ciphertext
  exactly as stored.
- Every chunk authenticates backup ID, payload path, envelope version, chunk
  number, and final-frame marker as associated data.
- A missing key, invalid tag, unsupported version, mismatched nonce, associated
  data, or framing fails closed. Restore decrypts only into a transient file;
  no plaintext is written to the requested destination before the envelope has
  authenticated completely.

## Consequences

The local keyring is a required restore dependency for encrypted backups,
unless the operator selects ADR 0006's `guardian-vault` explicitly via
`--vault-dir` — the same per-payload data key stored in a portable encrypted
file instead of the OS credential store. Cross-node recovery and portable
key export were an open question at the time this ADR was written;
delivered by ADR 0013's portable repository recovery key, which wraps every
payload's data key under a repository-wide key that can itself be exported
into a passphrase-protected offline bundle. Existing format-v1
unencrypted backups remain readable only through a compatibility path; new
live filesystem captures create format-v2 encrypted payloads and cannot
silently claim encryption.
