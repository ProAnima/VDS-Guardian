# ADR 0017: Password logins through a one-shot askpass broker

Status: accepted and implemented (2026-10). Fulfils the password requirement left open by ADR 0015.

## Context

Many VDS operators have an IP address, `root`, and a password; they do not have
a key pair. A backup tool that only accepts keys excludes them. ADR 0015 made
password authentication a required capability but allowed it only if the secret
never appears in argv, the environment, shell text, configuration, logs,
diagnostics or a temporary file, and rejected `sshpass`, terminal scraping and
disabling `BatchMode` without a broker.

System OpenSSH can receive a password from exactly two places: a terminal, or a
program named by `SSH_ASKPASS` (forced with `SSH_ASKPASS_REQUIRE=force`, which
works without a terminal). So the secret has to reach an askpass program that
OpenSSH starts itself, over a channel that is neither argv, environment nor a
file.

## Decision

1. **Loopback broker.** Before spawning `ssh`, the application starts a broker
   (`guardian-askpass::Broker`). It holds the password in zeroized memory,
   listens on `127.0.0.1` on an ephemeral port, and answers one request that
   presents a fresh 256-bit token. It stops after the first successful answer,
   when dropped, or after `ConnectTimeout + 30 s`.
2. **The environment carries no secret.** `ssh` is started with
   `SSH_ASKPASS=<helper>`, `SSH_ASKPASS_REQUIRE=force`, the broker port and the
   token. The port and token authorise one request for a password; they are not
   the password.
3. **The helper is the application's own executable.** Desktop, `guardian-cli`
   and `guardian-mcp` call `guardian_ssh::init_password_helper()` first in
   `main`. When OpenSSH starts the executable as its askpass program, it
   answers the prompt and exits; otherwise it registers itself as the helper
   and the program starts normally. A password identity fails closed
   (`AskpassUnavailable`) until a helper is registered, so a binary that does
   not answer askpass calls can never be selected by accident. There is no
   separate sidecar to package or sign, and the program OpenSSH runs is the
   signed product.
4. **The helper only answers the login prompt.** It refuses everything except
   `<user>@<host>'s password:` or `Password:`: key passphrases, host-key
   questions, `(current) UNIX password:`, `New password:` and one-time-code
   prompts get no answer, so the stored password is never typed into the wrong
   place or used to change a password.
5. **Exactly one attempt, host pinned first.** A password login sets
   `BatchMode=no`, `PasswordAuthentication=yes`, `PubkeyAuthentication=no`,
   `KbdInteractiveAuthentication=no`, `PreferredAuthentications=password` and
   `NumberOfPasswordPrompts=1`. `StrictHostKeyChecking=yes` with the pinned
   `known_hosts` is unchanged, and OpenSSH verifies the host key before it
   authenticates, so the password is never offered to a host that does not match
   the pin. A wrong password is not retried, which also avoids account lockouts.
6. **Storage.** The password is stored like a private key, under the profile's
   random credential id, in the OS credential store (or the encrypted vault
   fallback) as a `PASSWORD-V1` marker (base64 of 1–256 UTF-8 bytes without
   NUL, CR or LF). It never enters a profile document, settings export,
   repository metadata, log or diagnostic. Profiles record no secret.
7. **Typed through the whole stack.** Every `SystemOpenSsh` operation takes an
   `&SshIdentity` (key file, agent public key, or password) instead of a bare
   path, so capture, browse, Docker inventory, SQLite, restore, deploy and the
   preflight all support it identically.

## Rejected alternatives

- `sshpass`, `expect`-style terminal scraping, or `SSH_ASKPASS` scripts that
  embed or read the password from a file: forbidden by ADR 0015 and they leak.
- Putting the password in the askpass environment: other same-user tools can
  read it, and it can reach logs and diagnostics.
- A bundled sidecar helper (Tauri `externalBin`): extra signed binary, a build
  that needs prior preparation, and a second program to trust.
- A named pipe / Unix-socket channel: no stronger against a same-user attacker
  than a one-time token on loopback, and needs per-OS code and ACL handling.
- A native Rust SSH library: replaces the audited OpenSSH transport that every
  adapter, the host pinning, cancellation and streaming limits already rely on.
- Keyboard-interactive: left disabled; servers that accept passwords only
  through PAM keyboard-interactive are not supported yet.

## Consequences and residual risks

- A process running as the same OS user could read the OS credential store
  anyway; the broker adds no new same-user exposure beyond the short window in
  which the token sits in the environment of the `ssh` child.
- During authentication the password exists in the memory of the application,
  the broker thread (zeroized after serving or timeout), the helper, and the
  OpenSSH process. That cannot be avoided with system OpenSSH.
- A password is weaker than a key against online guessing and phishing of the
  operator; the UI recommends a dedicated backup user and warns when `root` is
  used with a password (it still works).
- Servers that require a password change on login, two-factor prompts, or
  keyboard-interactive-only password auth are not supported and fail closed.
- Registering the helper is per process; the stand-alone `guardian-askpass`
  binary exists for tests and tooling only.

## Evidence

- `guardian-askpass`: prompt filtering, wrong or absent token, single use,
  drop and lifetime, and the real helper binary (stdout carries exactly the
  password; refusals print nothing).
- `guardian-ssh`: password arguments and environment contain no secret, a key
  login is unchanged, marker validation, helper registration.
- Clean-room drill against a real `sshd` allowing `root` + password (run with
  the stand-alone helper and with the release desktop executable): a full
  capture over a password login seals an encrypted backup and the password
  appears nowhere in the repository; a wrong password is attempted exactly
  once; a host whose key does not match the pin never sees a password attempt.
