# ADR 0006: Encrypted local file vault for headless secret storage

## Status

Accepted.

## Context

`guardian-core::SecretStore` has exactly one production implementation:
`guardian-os-keyring::OsCredentialStore`, backed by Windows Credential
Manager or Linux Secret Service. Secret Service normally needs a logged-in
desktop session's D-Bus bus. A headless Linux VDS running `guardian-cli`
unattended (cron, a systemd timer, or a bare interactive shell with no
session bus) typically has none, so no secret — not the SSH identity key,
not the per-payload AES key ADR 0004 now mandates for every capture — can be
stored or loaded there today. This gap is already named in
`docs/adr/0002-secret-and-server-access.md` ("Headless Linux installations
need a documented Secret Service or encrypted vault fallback before
unattended scheduling is production-ready") and `docs/SIGNING_IDENTITY.md`,
but neither designs it.

## Decision

- A new adapter crate, `guardian-vault`, implements `SecretStore` as a local
  encrypted file store: one AES-256-GCM-CHUNKED-encrypted file per credential
  (`<vault-dir>/secrets/<credential-id>.enc`), reusing the existing streaming
  envelope from `guardian-encryption` unchanged rather than inventing a
  second crypto construction. Associated data binds a fixed domain constant
  and the credential id to each envelope, so an attacker with filesystem
  access cannot silently swap two credentials' ciphertexts.
- A single master key (`<vault-dir>/vault.key`, 32 CSPRNG bytes) encrypts
  every secret. It is protected by owner-only file permissions (Unix `0600`
  set at file creation; Windows ACL hardening via the same `whoami`/`icacls`
  pattern already used for the SSH identity temp file and restore scratch
  files) — **not** a passphrase. Unattended scheduling is the entire point of
  this fallback, and no human is present at boot or cron time to type one in.
  This is judged an equivalent trust boundary to what the OS backends already
  provide, not a lesser one: Windows Credential Manager (DPAPI) and Linux
  Secret Service both decrypt for any process running as the same logged-in
  account, exactly the blast radius a `0600` keyfile owned by that account
  has. A fixed-plaintext canary entry, written once at `init` and decrypted
  on every `open`/`status`, is the only structural check available for an
  otherwise-opaque 32-byte key (for example, an operator copying the wrong
  node's `vault.key` into place).
- Selection is explicit and per-invocation, never automatic: a new
  `--vault-dir <path>` flag on `guardian-cli credential`, `restore`, and
  `signing` opts into the vault; omitting it keeps today's OS-store behavior
  byte-for-byte. If a supplied `--vault-dir` fails to open, the CLI fails
  closed — it never silently falls back to the OS store, which would be a
  silent, security-relevant behavior change. A new `guardian-cli vault
  init|status` subcommand bootstraps and inspects a vault, mirroring
  `signing status`/`enroll`'s read-only-status / explicit-enrollment split.
  `init` never regenerates an existing master key (which would silently
  orphan every secret already encrypted under it); it recovers a previous
  `init` that wrote the key but was interrupted before the canary, without
  ever re-running key generation.
- `guardian-capture`, `guardian-local-repository`, and `guardian-signing`
  need no changes: they already depend on `&dyn SecretStore`, not the
  concrete OS-keyring type. Only `guardian-cli`'s dispatch changes. The
  Tauri desktop app is untouched — it always runs in a real user session, so
  it has no gap to close here.

### Alternatives considered and rejected

- **systemd `LoadCredentialEncrypted=`**: TPM-backed where available and a
  good fit for systemd-timer-scheduled Linux nodes specifically, but
  Linux-only (this project is Windows/Linux symmetric), only helps a process
  actually launched as a configured systemd unit (not a bare cron job or
  manual CLI run), and depends on Milestone 5 scheduler integration that
  does not exist yet. A reasonable future enhancement, not a substitute for
  a baseline that works today on either OS.
- **Linux kernel keyrings**: RAM-only; cleared on reboot unless a login
  session reseeds them, which just relocates the "nobody is logged in"
  problem this fallback exists to solve.
- **TPM sealing**: binds the encrypted blob to PCR measurements, so a routine
  kernel, firmware, or bootloader update can silently brick the vault and
  lock the operator out of their own backups — an unacceptable availability
  risk for a disaster-recovery tool. It also needs a TPM 2.0 chip many VDS
  hosts do not expose, a new heavy dependency, and a separate Windows CNG
  code path.
- **Optional passphrase**: does not help the motivating scenario at all (no
  human is present at boot/cron time) and adds real cost — KDF choice, salt
  storage, rotation, prompt-or-not UX — for no gap-closure.

## Consequences

- A headless Linux (or Windows) node with no usable OS credential store can
  now enroll SSH/signing credentials and create encrypted backups by opting
  into `--vault-dir`, closing a top P0 finding from the 2026-07-14
  production-readiness audit.
- Zero new external dependencies: `guardian-vault` uses only crates already
  in the workspace (`guardian-encryption`, `fs2`, `serde`, `zeroize`,
  `rand_core`, `thiserror`). `guardian-encryption` gains one small additive
  function, `decrypt_self_describing_reader_to`, for envelopes with no
  external, independently-authenticated nonce record to cross-check against
  (unlike payload decryption, which cross-checks the signed manifest) —
  `decrypt_reader_to`'s signature and behavior are unchanged.
- Residual risk, read: anyone with owner-level filesystem read access to the
  vault directory can decrypt every secret in it — the same blast radius the
  OS credential store already has within one account, not a new weakness.
- Residual risk, write: without a freshness or generation counter, an
  attacker with write access to the vault directory could roll back one
  credential's `.enc` file to a previously-valid value and it would still
  decrypt successfully. Detecting this is future work, not blocking here.
- Not delivered by this change: passphrase/KDF-protected vaults, TPM or
  hardware-token sealing, systemd-creds integration, a tool to migrate
  secrets between the OS store and the vault (an operator re-enrolls against
  whichever store they select), and vault key rotation.
