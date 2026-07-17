# Pinned SSH Capture Foundation

`guardian-ssh` is the deliberately narrow system-OpenSSH adapter used by the
shared capture and deploy compositions. It is not itself a CLI or Tauri command,
and its implementation alone does not make the product production-ready; the
Release 0.1 gates in `DEVELOPMENT_PLAN.md` still decide readiness.

## Trust input

The caller supplies the exact host, port, key algorithm, and base64 public host
key obtained through an explicit enrollment workflow. The adapter accepts only
Ed25519 and ECDSA host keys, validates that the SSH key blob declares the same
algorithm, and writes precisely that identity to a temporary `known_hosts`
file. It never uses accept-new mode or the operator's global known-host file.

## Invocation policy

The system `ssh` executable receives direct local argv, never a locally
constructed shell command. Its options disable password and keyboard-interactive
authentication and require `StrictHostKeyChecking=yes` and `IdentitiesOnly=yes`.
The capture composition resolves the profile credential reference through the
injected OS credential store. It accepts an unencrypted OpenSSH key envelope or
an unencrypted PEM private key (RSA, EC, or PKCS#8), writes it to a short-lived temporary identity
file, and deletes that file after the SSH invocation; private key bytes are
never logged or written to repository configuration. Windows temporary identity
files have inherited ACLs removed and grant access only to the current user.

A passphrase-protected key is supported through an already-running OS SSH
agent (ADR 0009), never by VDS Guardian handling the passphrase itself. The
credential store holds a small self-describing public-key marker instead of
raw key bytes for this case; the resolved identity is written only as a
`.pub` file with no private-key-shaped path alongside it, relying on
OpenSSH's own documented fallback (read the public key from `<path>.pub`,
then ask the agent to sign) rather than any new SSH flag. Registered
through `guardian-cli credential register-agent-key` today; desktop UI is
not yet wired up. Limited to `ssh-ed25519`/`ecdsa-sha2-nistp256/384/521`
identities in this first slice.

The only current remote command template is a read-only GNU tar stream:

```text
tar --create --file=- --zstd --numeric-owner --one-file-system -- <roots>
```

Capture roots must be absolute, lexical paths without traversal or control
characters. Each root is single-quote encoded for the remote shell and follows
`--`, so a root cannot be treated as a tar option.

## Capability probe

`probe_tar_zstd` uses the same pinned host key, identity file, and noninteractive
SSH arguments as capture. It runs the fixed command below, with all output
discarded, and returns only whether it exited successfully:

```text
LC_ALL=C tar --create --zstd --file=/dev/null --files-from=/dev/null >/dev/null 2>&1
```

The probe creates no remote files and receives no operator-controlled command or
path. SSH has a 30-second connect timeout and a 15-minute total deadline by
default. Capture streams have a five-minute idle-byte deadline: no received
archive bytes within that period terminates local SSH and removes the partial
file. Operator-triggered cancellation (ADR 0010) now stops a capture in
progress: the desktop app's Cancel affordance signals a cross-thread handle
this loop polls between reads, and the spawned child is placed in its own
process group so only that cooperative signal, not a raw OS interrupt
racing it, ends it.

## Docker inventory command

The Docker adapter may invoke only this fixed read-only command through the
same SSH policy:

```text
ids=$(docker ps --all --quiet --no-trunc) || exit 1; [ -z "$ids" ] || printf '%s\n' "$ids" | xargs -r docker inspect --
```

The local stream is capped at 8 MiB; exceeding the cap terminates SSH and
discards output. The JSON parser treats all returned metadata as hostile before
it can enter the inventory use case.

## Failure behavior

The output destination is created exclusively. Each OpenSSH invocation has a
bounded total runtime (15 minutes by default, configurable only by the
composition root). If OpenSSH cannot start, exceeds that deadline, or exits
unsuccessfully, its local process is killed where needed and the partial stream
is removed. Operator-triggered cancellation now covers capture (ADR 0010) on
top of this deadline-based termination. The capture composition
inspects the completed tar.zst stream, hashes it from disk, and registers it
with staging; manifest finalization and sealing remain separate fail-closed use
cases.
