# ADR 0018: Host-key lookup with fingerprint confirmation

Status: accepted and implemented (2026-10).

## Context

Every server profile pins the server's public host key, and a changed key fails
closed. Until now the operator had to obtain the whole public key (for example
`ssh-keyscan` or a provider panel) and paste it, which most operators cannot do
reliably. The security model already promised: "first connection shows the
fingerprint and requires explicit trust."

## Decision

1. **Retrieval only.** The Servers form has a "fetch host key" action. It asks
   the entered address and port for the key the server presents and shows the
   standard OpenSSH fingerprint (`SHA256:` plus unpadded base64, exactly what
   `ssh-keygen -l` and provider panels print). It pins nothing and trusts
   nothing.
2. **Trust is an explicit operator act.** The existing acknowledgement stays
   mandatory and its wording changes to "I compared this fingerprint with one
   from my provider or another trusted channel". The acknowledgement is reset
   whenever a new key is fetched or typed, and a fetched key is discarded when
   the address or port changes, so a key can never be confirmed for one server
   and pinned to another. The backend does not rely on the form:
   `enroll_ssh_profile` refuses a request without the acknowledgement
   (`host_key_unconfirmed`), and when the form sends the fetched fingerprint it
   must be the fingerprint of exactly the key being pinned
   (`host_key_fingerprint_mismatch`, `require_host_key_confirmation`).
3. **How the key is fetched.** System `ssh` connects with no credential
   (`PubkeyAuthentication=no`, `PasswordAuthentication=no`,
   `KbdInteractiveAuthentication=no`, `BatchMode=yes`, `ProxyCommand=none`,
   `-F none`), records the key into a throw-away `known_hosts` file as soon as
   it is received (`StrictHostKeyChecking=no` here only means "record without
   asking") and ends when authentication has nothing to offer. The preferred
   algorithm order is ed25519, then ECDSA P-521/384/256. The key is parsed and
   validated by the same `HostPin` rules as a pasted key.
4. **Argument safety.** The host passes the same strict host-name validation as
   a pin (no leading `-`, only letters, digits, `.` and `-`) and the program is
   run with direct argv, never a shell.
5. **Desktop only.** The lookup is a desktop command. It is not exposed through
   `guardian-mcp` or the CLI, consistent with ADR 0012: an agent must not be
   able to mint host trust from content it read elsewhere.

## Rejected alternatives

- `ssh-keyscan`: the Windows build cannot negotiate the post-quantum key
  exchange that current OpenSSH servers prefer, so it fails on modern servers
  (observed against OpenSSH 9.7). `ssh` itself supports every exchange the
  pinned connections already use.
- Pinning the fetched key automatically (trust on first use without a
  comparison): converts a convenience into an unauthenticated trust decision.
- Showing the existing internal digest: it is a SHA-256 of the base64 text, not
  the standard fingerprint operators can compare.

## Consequences and residual risks

- A network attacker present during this single lookup could present their own
  key. This is the unavoidable first-contact risk of any host key lookup; the
  mandatory out-of-band fingerprint comparison is the control. Operators who
  cannot compare may still paste a key they obtained by another trusted route.
- The lookup appears in the server's log as one failed pre-authentication
  connection from the operator's machine.
- The lookup is bounded: an 8-second connect timeout and at most 11 seconds
  for the `ssh` process overall, 16 KiB of recorded key material, no
  credential, no retries.

## Evidence

Unit tests cover fingerprint equality with `ssh-keygen`, parsing and algorithm
preference, malformed and unsupported keys, host validation, and the argument
set; desktop command tests cover the safe error mapping and the backend
confirmation check (no pin without the acknowledgement, a mismatched confirmed
fingerprint is refused). A clean-room drill
(`crates/guardian-capture/tests/clean_room_drill/host_key_scan.rs`) against a
real `sshd` checks that the scanned key is
the server's own and that the fingerprint equals an independent `ssh-keygen -l`
of that key, and that a closed port fails quickly. Front-end tests cover
fetch, reset of the confirmation, discarding a key on address change, manual
edits, and failure.
