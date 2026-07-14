# Pinned SSH Capture Foundation

`guardian-ssh` is a deliberately narrow adapter for a future backup use case.
It is not a CLI command, Tauri command, or production-ready backup workflow.

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
The caller provides a key-file path by reference; private key bytes are not
stored or logged by the adapter.

The only current remote command template is a read-only GNU tar stream:

```text
tar --create --file=- --zstd --numeric-owner --one-file-system -- <roots>
```

Capture roots must be absolute, lexical paths without traversal or control
characters. Each root is single-quote encoded for the remote shell and follows
`--`, so a root cannot be treated as a tar option.

## Failure behavior

The output destination is created exclusively. If OpenSSH cannot start or exits
unsuccessfully, the partial stream is removed. A future use case must inspect
the completed tar.zst stream, hash it, add it to the manifest, and seal it only
after all verification passes.
