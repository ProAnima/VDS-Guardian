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
   presents a fresh 256-bit token (compared in constant time). Its lifetime is
   the SSH connect timeout plus a 30-second grace (60 s with the default 30 s
   connect timeout); it stops accepting at the end of that lifetime, after the
   first successful answer, or when the prepared `ssh` command that owns it is
   dropped. Each connection gets a 1.5-second budget to present the token and
   receive the answer, at most 8 connections are handled at once (further ones
   are closed immediately), and only loopback peers are served. A wrong token
   is denied without consuming the password.
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
   `<user>@<host>'s password:`: key passphrases, host-key questions, a bare
   keyboard-interactive `Password:`, `(current) UNIX password:`,
   `New password:` and one-time-code prompts get no answer (and the broker is
   never contacted), so the stored password is never typed into the wrong
   place or used to change a password. It also answers only when started the
   way OpenSSH starts an askpass program (one prompt argument,
   `SSH_ASKPASS_REQUIRE` set, port and token present); stray variables in a
   user's environment are ignored.
5. **Exactly one attempt, host pinned first.** A password login sets
   `BatchMode=no`, `PasswordAuthentication=yes`, `PubkeyAuthentication=no`,
   `KbdInteractiveAuthentication=no`, `PreferredAuthentications=password` and
   `NumberOfPasswordPrompts=1`. `StrictHostKeyChecking=yes` with the pinned
   `known_hosts` is unchanged, and OpenSSH verifies the host key before it
   authenticates, so the password is never offered to a host that does not match
   the pin. A wrong password is not retried, which also avoids account lockouts.
6. **Storage.** What is stored is the credential bytes
   `PASSWORD-V1\n<base64 of the password>\n` (1–256 UTF-8 bytes without NUL,
   CR or LF), under the profile's random credential id, exactly where a
   private key would be. Only the desktop Servers form creates one, and the
   desktop always uses the OS credential store. The CLI has no command that
   stores a password (`credential import-ssh-key` rejects a password marker,
   and a password is never read from a key file); CLI and MCP can log in with
   a password credential that already exists in the store they read, and the
   encrypted vault (`--vault-dir`) can hold one only if it was put there by
   other means, as the clean-room drill does. The password never enters a
   profile document, settings export, repository metadata, log or diagnostic.
   The profile records only the non-secret `auth_kind: password`.
7. **Typed through the whole stack.** Every `SystemOpenSsh` operation takes an
   `&SshIdentity` (key file, agent public key, or password) instead of a bare
   path, so capture, browse, Docker inventory, SQLite, deploy, managed source
   replacement and the preflight all support it identically.

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
- The broker port is reachable by other local users on the same machine.
  Without the token they cannot obtain the password, and a wrong token does
  not consume it, but they can keep the eight connection slots busy and make
  one login fail: a local denial of service, not a disclosure.
- During authentication the password exists in the memory of the application,
  the broker (zeroized when the broker is dropped), the helper, and the
  OpenSSH process. That cannot be avoided with system OpenSSH.
- At enrollment the password typed in the desktop form passes through WebView
  JavaScript memory and the Tauri IPC request before Rust receives it; only
  the Rust copy is zeroized. Later logins read it from the credential store in
  Rust and never send it back to the WebView.
- A password is weaker than a key against online guessing and phishing of the
  operator; the UI recommends a dedicated backup user and warns when `root` is
  used with a password (it still works).
- Servers that require a password change on login, two-factor prompts, or
  keyboard-interactive-only password auth are not supported and fail closed.
- Registering the helper is per process; the stand-alone `guardian-askpass`
  binary exists for tests and tooling only.

## Evidence

- `guardian-askpass` unit tests (`src/lib.rs`): prompt filtering, a wrong
  token is denied without consuming the password, single use, dropping the
  broker closes the port, expiry after the lifetime, a prompt mismatch never
  contacts the broker, stalled local clients neither block the legitimate
  request nor delay dropping the broker, token format, and password length
  bounds.
- `crates/guardian-askpass/tests/helper_binary.rs`, against the compiled
  helper: stdout carries exactly the password and a newline; other prompts,
  missing configuration, a malformed token, and stray variables without the
  OpenSSH call shape produce empty output; the password never appears on
  stderr.
- `crates/guardian-ssh/tests/password_login.rs` and the `guardian-ssh` unit
  tests: password-only options with one attempt, host pinning unchanged and no
  `-i`, the password in no argument, key logins unchanged, the environment
  carries only the helper, port and token, a fresh token per command, dropping
  the command closes its broker, key commands never touch the askpass
  environment, and marker round-trip/corruption handling (including refusing
  a password marker from a key file).
- `apps/desktop/src-tauri/src/profile_commands.rs` tests: the password is
  redacted from debug output and unusable passwords are rejected.
- Clean-room drill (`crates/guardian-capture/tests/clean_room_drill/password_login.rs`,
  `npm run test:integration:drill`) against a real `sshd` allowing `root` +
  password, with the credential in the encrypted vault: a full capture over a
  password login seals an encrypted backup and the password appears nowhere in
  the repository; a wrong password is attempted exactly once and seals
  nothing; a host whose key does not match the pin never sees a password
  attempt. It uses the stand-alone helper by default; `GUARDIAN_DRILL_ASKPASS`
  runs the same cases with another executable as the helper, such as the
  release desktop binary.
