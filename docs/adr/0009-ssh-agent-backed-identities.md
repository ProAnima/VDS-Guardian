# ADR 0009: SSH-agent-backed identities for encrypted keys

## Status

Accepted.

## Context

Milestone 2 names "encrypted-key/agent support" as a P0 exit-gate item.
Today `guardian-ssh` rejects any passphrase-protected SSH key outright:
`secret_identity.rs`'s `validate_openssh_envelope` parses only the
OpenSSH-v1 envelope header and requires the cipher/kdf/kdfoptions fields to
be exactly `"none"`/`"none"`/`""` — anything else (a real passphrase) fails
closed as `SshError::InvalidCredential` before a connection is ever
attempted. Zero ssh-agent scaffolding existed anywhere in the codebase.

An operator who wants to use a passphrase-protected key has one real
option in practice: keep the decrypted key resident in an already-running
`ssh-agent` (or, on Windows, the OpenSSH Authentication Agent service) and
let the SSH client ask the agent to sign, rather than ever handling the
passphrase or decrypted key material itself. This ADR covers only that
path — VDS Guardian never sees, prompts for, stores, or manages a
passphrase anywhere.

## Decision

### No new argv, no new plumbing to a socket or pipe

OpenSSH's `-i <path>` (`IdentityFile <path>` in config terms) has a
long-standing fallback: if `<path>` does not exist but `<path>.pub` does,
`ssh` reads the public key from `.pub`, uses it to select the matching
identity among whatever the agent already offers, and asks the agent to
sign — the private key never touches this process or this disk. Agent
discovery itself is ambient: Unix reads `SSH_AUTH_SOCK`; Windows OpenSSH's
own client already falls back to the well-known named pipe
`\\.\pipe\openssh-ssh-agent` when `SSH_AUTH_SOCK` is unset, driven by the
"OpenSSH Authentication Agent" service. Neither needs new code here, and
none of `guardian-ssh`'s existing flags (`-F none`, `BatchMode=yes`,
`PreferredAuthentications=publickey`, `IdentitiesOnly=yes`) interferes with
either path — agent-based auth is still publickey auth on the wire, and
`IdentitiesOnly=yes` still means "only the one identity I named," it just
doesn't change *how* that identity signs.

Consequence: `SystemOpenSsh::arguments_for_command` needed zero changes.
`-i <path>` and `IdentitiesOnly=yes` stay unconditional in both modes —
only what is materialized *at* that path differs. This is what kept this
change's footprint small: the whole feature lives in identity
*resolution*, not in the already-reviewed argv-building template.

### No `VdsProfile`/`SecretStore` schema change

`credential_id: CredentialId` and the generic `SecretStore` trait are
unchanged. `SecretStore` is shared infrastructure — also used for
manifest-signing seeds (`guardian-signing`) and per-backup payload
encryption keys (`guardian-local-repository`) — and must not gain
SSH-specific branching. The mode discriminator lives *inside* the opaque
secret blob itself, interpreted only by `guardian-ssh`; `guardian-core`,
`VdsProfile`, and every already-enrolled profile's `profiles.json` are
unaffected. This still honors ADR 0002's invariant ("application
configuration stores credential references, never private key bytes or
passphrases") — the profile continues to hold nothing but a reference; the
new marker inside the referenced secret is a public key, never a
passphrase or private key.

### A self-describing marker, not a private key

A fixed text marker distinguishes an agent-backed identity from a real
private key (which always starts with a PEM header):

```text
AGENT-IDENTITY-V1
<algorithm>
<public-key-base64>
```

`<algorithm>` is restricted to the same allowlist `HostPin` already uses
(`ssh-ed25519`, `ecdsa-sha2-nistp256/384/521`) — RSA agent identities are
out of scope for this change, not a silent gap. `<public-key-base64>` is
validated as a well-formed RFC 4253 §6.6 public-key blob, the same shape
`HostPin::validate` already checks. This validation is deliberately
duplicated locally in `guardian-ssh` rather than shared with
`guardian-core`: the marker is guardian-ssh's own internal storage format
and never crosses into core domain code, so a few duplicated lines are a
smaller, safer footprint than reworking already-reviewed code for a
concern that belongs to one crate.

`SecretIdentityFile` (a struct) became `SshIdentity` (an enum:
`PrivateKey(TempPath)` / `AgentPublicKey { pub_file, identity_path }`).
Classification checks for the marker's fixed first line before falling
through to the existing PEM/OpenSSH-envelope detection, completely
unchanged. The agent variant writes only a `<random>.pub` file (via
`tempfile::Builder::new().suffix(".pub")`, so the real filesystem name
already carries the suffix) containing `"<algorithm> <public-key-base64>"`
— OpenSSH's own `.pub`-file text format — with no file at the base path at
all. That `.pub` file gets the same permission hardening a private-key
file does, for consistency, even though its content is not secret. Every
one of the seven existing call sites (`guardian-ssh`, `guardian-docker`,
`guardian-deploy`, `guardian-database` ×2, `guardian-capture` ×2, and the
desktop's `profile_commands.rs`) only ever calls `.path()` and hands the
result to `-i` — none of them needed a behavior change, only the type
name.

A malformed marker (disallowed algorithm, corrupt base64, wrong line
count, an algorithm string that does not match its own embedded blob)
fails closed as `SshError::InvalidCredential` — the same variant already
used for a malformed private key; no new variant was needed.

### New CLI command, no desktop UI yet

`guardian-cli credential register-agent-key --credential-id <id>
--public-key-file <absolute-path> [--vault-dir <absolute-path>] --json`
reads a standard `.pub` file, validates it, converts it to the marker
above, and stores it — reusing `import-ssh-key`'s existing
lock/no-overwrite/store/read-back-verify sequence unchanged rather than
duplicating it. An agent-mode profile is then authored exactly like any
other CLI-only profile already is: `register-agent-key` followed by a
hand-authored `profile enroll --input <profile.json>` referencing that
credential ID — the existing two-step flow, unchanged, since `VdsProfile`
never changes shape.

Desktop UI (a picker for "stored key file" vs. "SSH agent, provide a
public key" in `SshProfilePanel.tsx`) is explicitly deferred to a later,
separately approved slice — matching how ADR 0005 (embedded-database
adapter) and ADR 0008 (Docker mount capture) both shipped their adapter
first and left UI wiring to a later slice.

### Real-world failure needs no new handling

If the agent is not running, or does not hold the matching key, `ssh`
simply fails authentication — the exact same connection-failure path every
other SSH failure already takes. Nothing new to detect or handle; this
fails closed automatically via existing machinery.

## Consequences

- Passphrase-protected keys are now usable for capture, restore-adjacent
  probes, and deploy, provided the operator keeps the matching key loaded
  in an already-running OS agent — VDS Guardian never sees the passphrase.
- No migration, no `Document.format_version` bump, no risk to any
  already-enrolled profile: the entire feature is additive at the
  `guardian-ssh` layer.
- Two real residual gaps, named rather than silently dropped: RSA agent
  identities are out of scope this slice, and there is no desktop UI yet
  to author an agent-mode profile without hand-editing a JSON document via
  the CLI.
- A live ssh-agent round trip is not exercised by any automated test in
  this repository yet (the same category of gap the clean-room drill
  closed for the raw-key path) — covered instead by marker-classification
  and CLI-command unit tests. A follow-up could extend the clean-room
  drill's disposable fixture with a real `ssh-agent`/`ssh-add` step for
  full end-to-end coverage.
