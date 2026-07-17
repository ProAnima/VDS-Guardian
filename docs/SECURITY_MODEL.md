# Security Model

## Assumptions

- The remote VDS may be compromised, malicious, or partially unavailable.
- Backup content may contain malware, hostile filenames, symlinks, devices,
  decompression bombs, and secrets.
- The operator machine and backup repository are trusted at installation time,
  but ransomware or credential theft remain possible.
- SSH provides transport security only after host identity is pinned correctly.
- A successful backup is not proof that the captured application is healthy.

The clean-room restore drill includes a sealed, authenticated archive whose
tar metadata contains a `../` escape path. It must be rejected by archive
inspection after decryption and before a restore destination is published.

## Primary assets

- SSH private keys and passphrases.
- Backup payloads and manifests.
- Backup-node signing identity.
- Server profiles and pinned host keys.
- Audit records and restore approvals.
- Availability of at least one clean, independently stored recovery point.

## Trust boundaries

1. React WebView to Tauri bridge.
2. Application core to OS/process/SSH adapters.
3. Backup node to remote VDS.
4. Staging directory to sealed repository.
5. Sealed repository to restore target.
6. Installed binary to update/release infrastructure.
7. Sealed repository to remote deploy target (a new/different VDS).
8. MCP client (an external tool or AI agent) to `guardian-mcp`, over stdio
   only (ADR 0012) — the client is whatever local process launched
   `guardian-mcp` as its own child, the same OS-process trust `guardian-cli`
   and the desktop app already assume; this boundary is not network-reachable
   and does not widen who can trigger capture/restore/deploy.

## Mandatory controls

### Credentials

- Store secrets in OS credential storage, referenced by random credential ID.
- Never embed a private key in the repository, application resources, logs, or
  portable configuration exports.
- Support operator-selected key files only by reference and validate restrictive
  permissions where the platform supports them.
- Prefer a dedicated backup account with least privilege and reviewed `sudo`
  commands over unrestricted root login.
- Desktop SSH enrollment stages a new credential only long enough to run the
  pinned `tar.zstd` preflight. The profile becomes visible to capture and setup
  readiness only after that probe succeeds; probe or profile-commit failure
  removes the staged credential, and cleanup failure is itself a hard error.
- Desktop profile deletion requires an explicit per-card confirmation and is
  refused while a saved capture plan references that profile. The profile
  document is rewritten atomically before its credential is removed; if secure
  credential cleanup fails, the profile is restored so the UI never silently
  leaves a selectable profile without its key.
- Password-based SSH is unavailable until a native adapter or one-shot askpass
  broker delivers the password through memory-only local IPC. Password bytes
  must never enter argv, environment variables, shell input, configuration,
  logs, diagnostics, or temporary files. `sshpass` and terminal-prompt scraping
  are forbidden. Host-key pinning and capability preflight remain mandatory.

### Remote browsing and selection

- Directory browsing is read-only, paginated, and bounded by entry count,
  metadata bytes, output bytes, and deadline. It lists one validated absolute
  directory at a time and never recursively scans a server implicitly.
- Browsing treats names, types, timestamps, sizes, cursors, and transport output
  as hostile. Symlinks may be displayed but are never followed or selectable as
  an implicit target. Sockets, devices, and other special entries are not
  capturable through the explorer.
- Page cursors bind their offset to a digest of the sorted listing. A changed
  listing rejects a stale cursor instead of silently mixing directory states.
- The adapter accepts a typed directory path, never an operator command or
  command fragment. Any fixed-command implementation requires adversarial
  quoting/output-parser tests before use; SFTP implementations require the same
  pinned-host and bounded-output guarantees.
- Docker container/group selection resolves only to validated capturable mount
  paths. The preview displays the resolved host paths and consistency warnings;
  execute resolves and validates them again instead of trusting UI metadata.
- The implemented preview rejects client-supplied Docker paths that do not
  exactly match the current validated inventory, and binds its confirmation
  identity to the logical items and normalized roots. This preview is not yet
  an execution authorization gate; that revalidation remains required before
  direct selection execution is exposed.

### Encrypted local vault fallback

- A headless node with no usable OS credential store (typically Linux with no
  logged-in session bus for Secret Service) can opt into `guardian-vault`, a
  local encrypted file store implementing the same `SecretStore` contract
  (ADR 0006). Selection is explicit and per-invocation via `--vault-dir` on
  `credential`/`restore`/`signing`; omitting the flag keeps the OS store as
  the default, and a vault that fails to open is a hard failure, never a
  silent fallback to the OS store.
- Each credential is its own AES-256-GCM-CHUNKED-encrypted file, reusing the
  existing payload envelope unchanged. Associated data binds a fixed domain
  constant and the credential id, so ciphertexts cannot be silently swapped
  between credentials. A single master key file, owner-only permissioned
  (Unix `0600` at creation; Windows ACL hardening via the same pattern used
  for the SSH identity temp file), protects every entry — no passphrase,
  since unattended scheduling is the reason this fallback exists.
- `vault init` never regenerates an existing master key; `vault status` is
  read-only and never creates the directory, key, or canary as a side
  effect. Owner-level filesystem read access to the vault directory
  discloses everything in it — the same blast radius the OS credential store
  already has within one account, not a new weakness.

### Portable repository recovery

- Every repository has, since ADR 0013, one recovery key that wraps every
  payload's primary data key as a second, additional copy signed into that
  payload's own manifest entry — the primary `SecretStore` reference is
  unchanged and remains the first key resolution always tries. Live capture
  fails closed before touching SSH if the target repository has no
  configured recovery key (`recovery init`), so an encrypted backup can
  never be sealed with no independent recovery path.
- `recovery export` wraps the repository recovery key itself under an
  Argon2id-derived key from an operator-supplied passphrase into a portable
  bundle file, bound via
  authenticated associated data to the specific repository id and active
  Ed25519 public verification key — a bundle
  exported from one repository fails closed if fed into another, even with
  the correct passphrase. `recovery import` reverses this on a clean
  machine, installing the recovered key into whatever `SecretStore` that
  machine is configured to use. The authenticated public key is pinned in
  repository metadata and verifies manifests without copying the private
  signing seed; a wrong passphrase, wrong repository, substituted key, or
  corrupted bundle all fail closed via the same AEAD authentication check,
  never a separate one. Authentication completes before an unregistered
  transferred repository is added to the local registry, so a wrong
  passphrase leaves neither imported key material nor a repository
  registration. Import accepts only the pinned format-v1 Argon2id
  cost profile, is idempotent for the same key, and refuses to overwrite a
  different working recovery key. The shared recovery service accepts only
  passphrase bytes from its adapter; the CLI reads them from a validated file,
  never a bare command-line argument. Desktop keeps them in memory only and
  requires two matching entries before export, preventing an unnoticed typo
  from producing the operator's only offline bundle.
- Both `recovery export` and `recovery import` require a typed confirmation
  phrase computed from the repository id, matching the confirmation-gate
  convention restore and deploy already use. Neither `guardian-mcp` nor any
  other headless surface exposes these tools — the single
  highest-blast-radius secret in the system is deliberately excluded from
  MCP the same way profile enrollment, vault init, and signing enrollment
  already are.
- The recovery bundle must be stored independently of the repository disk
  (an offline copy — a separate drive, a safe, a password manager) to serve
  its purpose; a bundle kept alongside the repository it recovers is a
  single point of failure again. Recovery-key rotation and bundle
  replacement remain open, deferred to Release 0.2.

### SSH

- First connection shows the fingerprint and requires explicit trust.
- Later fingerprint changes fail closed and require a separate re-enrollment
  workflow; no accept-new fallback in scheduled jobs.
- Use timeouts, keepalive, cancellation, output caps, and strict argument
  encoding. Do not upload and execute unversioned shell text from the UI.
- Capability discovery is read-only and becomes part of the backup plan.

The current `guardian-ssh` foundation accepts an exact pinned public host key,
writes it to a temporary `known_hosts` file, and invokes the system OpenSSH
client through direct local argv with `StrictHostKeyChecking=yes`, noninteractive
authentication, and global known-host lookup disabled. It accepts only a
validated backup user and an allowlisted read-only tar command template; remote
path arguments are independently shell-quoted. Every capture has a bounded
total runtime and removes its partial local stream after a launch error,
deadline, or nonzero exit. The capture composition resolves the profile's
credential reference through the injected secure store, accepts only an
unencrypted OpenSSH private-key envelope or unencrypted PEM private key, and deletes its short-lived temporary
identity file after SSH exits. Windows temporary identity-file ACLs are reduced
to the current user before SSH starts. The local capture destination is
narrowed to the current user (Unix `0600`; the same Windows ACL pattern) the
moment it is created, before any streamed byte reaches it — a captured
filesystem or database payload is arbitrary customer content held in
plaintext until it is later encrypted, a materially larger exposure than the
identity file it sits next to. Every file the local repository writes through
its shared atomic-write primitive (manifest, signature, verification report,
and any payload staged via the in-memory write path) receives the same
owner-only permissions. Encrypted-key/agent support is now implemented
(ADR 0009): a credential reference can hold a small self-describing public-
key marker instead of raw key bytes, resolved into a `.pub`-only identity
file with no private-key-shaped path beside it — the private key itself
never reaches this process, relying entirely on an already-running OS SSH
agent (or, on Windows, the OpenSSH Authentication Agent service) to hold
the decrypted key and perform the signature. VDS Guardian never prompts
for, stores, or otherwise sees the passphrase. Limited today to
`ssh-ed25519`/`ecdsa-sha2-nistp256/384/521` identities, registered only
through `guardian-cli credential register-agent-key`; desktop enrollment
UI is not wired up yet. Operator-triggered cancellation (ADR 0010) now
covers capture, deploy, and desktop local restore: the CLI installs a Ctrl+C
handler for deploy and the desktop app exposes a Cancel affordance backed by a
per-job registry, both
setting a cross-thread handle the transport polls between reads; the
spawned child is placed in its own process group so only that cooperative
signal, not a raw OS interrupt racing it, ends it. Local restore polls the same
handle during decryption, tar extraction, and SQLite decompression and again
before its single atomic publish; cancellation removes its fresh staging tree
and leaves no destination. Adversarial tests force cancellation mid-stream for
both archive forms and at the repository boundary. The Docker-backed drill now
proves real capture and deploy cancellation after the corresponding stream has
transferred its first byte: capture leaves no local staging or sealed backup;
deploy leaves no remote staging directory or target; both audit states are
`cancelled`. Capture
streams also have a five-minute idle-byte deadline that kills local SSH and
discards the partial stream regardless of whether cancellation was
requested. The adapter's fixed read-only `tar --zstd` probe uses the same pinned
identity and a 30-second SSH connect timeout. The shared preflight use case
requires that probe's success before capture can continue; its result alone
never authorizes a backup.

The filesystem capture composition does not expose a manifest-ready payload as
a successful backup result. It holds the staging handle through archive
inspection, payload registration, manifest finalization, signature verification,
and atomic seal. Reserve and finalization failures are audited and invoke the
same discard/quarantine path; a sealed backup is the only successful result.
Before it creates staging, that composition runs the same pinned read-only
`tar --zstd` preflight itself; a UI check cannot authorize or bypass a live
capture. Its OpenSSH stream has a 20 GiB compressed-output cap and requires at
least that budget plus a 5 GiB free-space reserve on the destination filesystem.
The capture is rejected before opening staging if the reserve is unavailable.

New live filesystem captures replace the inspected staging archive with a
streaming AES-256-GCM ciphertext before it can enter a sealed directory. A
fresh payload key is stored in the OS credential store (or, when selected via
`--vault-dir`, `guardian-vault`) under a random reference; ciphertext digest,
envelope version, nonce, algorithm, and that reference are signed in the
format-v2 manifest. Since ADR 0013, capture also requires the target
repository to have a configured recovery key and fails closed if it does
not: the same payload key is additionally wrapped under that repository-wide
recovery key and the wrapped copy is signed into the manifest alongside the
primary reference, so restore can recover it independently of the original
machine's credential-store state (see "### Portable repository recovery"
below). Failed staging cleanup removes payload files before quarantine so a
plaintext archive is never retained as a quarantine artifact. Restore
verifies the sealed ciphertext first, resolves the key through the primary
credential-store reference or, when that is unavailable, through the
manifest's recovery-wrapped copy, fully authenticates into a transient file,
and only then extracts to the requested new destination. Key rotation is
still open.

The desktop enrollment screen follows the same boundary: the operator supplies
an absolute path to a dedicated unencrypted OpenSSH or PEM private key and
explicitly confirms that the pasted host key was verified out-of-band. One
shared-core enrollment transaction validates the regular non-symlink key file,
stages its bytes in the OS credential store under a generated reference, and
runs the fixed pinned `tar --zstd` capability preflight before persisting any
public profile data. The probe never accepts an operator-supplied remote command
and a changed host key fails closed. A failed secret write, probe, or profile
commit removes the staged credential and publishes no profile. If the operating
system refuses that cleanup, enrollment returns a distinct hard error; an
unreferenced credential may remain locally, but it never becomes a usable
profile or exposes key bytes. The preflight remains read-only and does not
create a backup or authorize a live run on its own.

Desktop repository registration accepts only an absolute non-symlink directory
path, initializes the existing local repository layout, and records a public
repository ID, display label, and canonical path in an atomically replaced
local registry. It never stores server credentials or archive payloads in the
application configuration. If registration fails after repository initialization,
the repository remains isolated on disk but is not treated as a configured
backup target; cleanup/discovery is a future recovery workflow.

### Repository isolation

- Each run writes to `<repository>/staging/<run-id>` on the same filesystem as
  the final location, then seals by atomic rename to `<repository>/backups/<id>`.
- Normal APIs never open a sealed backup for write.
- Retention removes an entire backup directory; it does not rewrite survivors.
- A manifest lists every file, length, digest, media type, and logical role.
- Read-only flags are defense in depth, not an immutability guarantee. Strong
  ransomware resistance requires a second node, offline/removable media, or an
  object store with retention lock in a later milestone.

The implemented repository foundation enforces validated identifiers during deserialization,
slash-only relative payload paths, symlink rejection at write and verification
boundaries, a cross-process writer lock held for the staging lifetime, streaming
SHA-256 verification, Ed25519-only signing metadata, quarantine on seal failure,
and same-filesystem atomic rename. Ed25519 seeds are zeroized in memory and can
be persisted as binary secrets in Windows Credential Manager or Linux Secret
Service under a random credential ID. Golden fixtures pin canonical manifest
bytes. Enrollment orchestration now holds a cross-process configuration lock,
commits a credential reference atomically, and recovers the same key from a
durable non-secret intent after interruption. It is exposed through explicit
CLI and desktop commands, never implicitly.
Retention verifies canonical manifest bytes, Ed25519
signatures, and the exact payload tree before planning or executing a
snapshot-bound whole-directory deletion. Retention deletion now writes a
durable non-secret intent outside its temporary quarantine directory. On the
next repository open, a move-phase interruption is rolled back; a durable
cleanup-ready phase is resumed idempotently. Orphaned or malformed retention
state fails closed. Read-only hardening, key rotation, integration tests, and
clean-room restore drills remain mandatory before production use.
The compiled-CLI drill now mutates only a disposable copy of a sealed
repository and proves that an encrypted-payload authentication failure leaves
no published restore destination. It separately proves the same cleanup
property when the repository recovery key is absent. A separately sealed
fault fixture has a valid filesystem payload followed by a correctly signed
and encrypted but invalid database zstd stream; its compiled-CLI restore proves
that a late second-payload failure removes staging and publishes nothing.

Signing configuration tampering cannot silently select a replacement identity:
the configured public key ID must match the key loaded through its credential
reference. A missing committed secret, incompatible schema, unsafe metadata
file, or concurrent enrollment fails closed. The recovery journal contains only
a random credential ID and format version.

Status inspection cannot initiate enrollment. CLI enrollment requires an exact
verb, JSON mode, and an absolute configuration path. Tauri performs credential
work outside the UI thread, and both adapters return bounded error codes and
remediation text rather than internal paths or operating-system error payloads.
Process-local lock registries close Windows same-process re-entry while OS file
locks continue to serialize independent processes.

The desktop setup screen follows the same boundary: rendering, status refresh,
and browser preview cannot create an identity. Enrollment requires a separate
user action, an acknowledgement checkbox, and a final confirmation; cancelling
clears that acknowledgement.

### Hostile backup content

- Archive entry names use a dedicated cross-platform relative-path type that
  rejects absolute paths, traversal, empty segments, Windows separators/drive
  syntax, alternate streams, and NUL bytes before extraction exists. A real
  tar directory entry's own trailing slash (`srv/app/`, not `srv/app`) is
  stripped before this validation runs, but only for directory-type entries —
  a file entry ending in `/` is not real tar output and stays rejected (ADR
  0011, 2026-07-16; this closes a defect that had rejected every real
  captured directory since this validator was first written).
- Verification hashes bytes without executing or previewing them.
- The current streaming tar.zst inspector rejects unsafe paths and every entry
  type except regular files and directories, including links, device nodes, and
  other special files. It enforces entry-count, declared per-file, and expanded
  stream-byte limits before extraction exists.
- The initial extractor accepts only a new destination it creates itself, uses
  validated relative paths rather than archive-provided extraction helpers,
  never preserves ownership or permissions, and removes its partial
  destination on failure. Missing ancestor directories between the
  destination and an entry's own parent are created as needed (real tar never
  emits a separate entry for a path segment above a multi-segment capture
  root) and hardened identically to every other directory this extractor
  creates; this can never escape the destination, since every path component
  was already validated and no entry type other than a regular file or
  directory is ever extracted (ADR 0011). Depth and expansion-ratio limits
  remain required before general-purpose restore support.
- Restores and remote deploy extraction never preserve setuid/setgid bits or
  archive-recorded ownership by default: local restore's Rust-native
  extractor never applies filesystem ownership/permission metadata from the
  archive at all, and remote deploy's `tar --extract` invocation passes
  `--no-same-owner --no-same-permissions` explicitly rather than relying on
  the incidental privilege level of the SSH session.
- Optional antivirus integration (P2, not yet built) should be an adapter with
  timeout and clear `not-scanned` versus `clean` states, so that no scanner
  result upgrades trust by itself once it exists.

### Docker and database discovery

- Docker/Compose output is untrusted. Inventory parsers must bound container and
  nested metadata counts, validate every identifier and mount destination, and
  store secret references only, never secret values.
- The initial core inventory contract enforces these limits before inventory can
  enter a capture plan. Its `docker inspect` parser accepts only a bounded JSON
  response and maps fixed Docker fields into that contract. The pinned-SSH
  adapter accepts no operator command input, uses one reviewed `docker ps` /
  `docker inspect --` template, and kills an output stream above 8 MiB. Embedded-database
  consistency logic is implemented for a lightweight SQLite-style file (see below);
  PostgreSQL/MySQL consistency (beyond version preflight) remains unimplemented.
- A bind mount's or named volume's host-side data resolves to a capturable
  absolute path (ADR 0008) reusing the existing filesystem-capture mechanism —
  no new remote command. Three residual risks are intentionally unresolved,
  not silently assumed away: a volume's reported source path is trusted at
  face value with no separate driver check, so a non-`local` volume driver
  fails closed only if `tar` itself cannot read what is there; a raw capture
  of a live, actively-written volume has no consistency guarantee (no generic
  quiesce mechanism exists — prefer the embedded-database adapter for a live
  database instead of raw volume capture of the same data); and reading
  Docker's volume storage needs filesystem access this tool never requests,
  grants, or assumes — the same operator-arranged, least-privilege prerequisite
  already required for the embedded-database adapter's target file.
- Database dump preflight rejects missing, malformed, or major-version-mismatched
  PostgreSQL/MySQL server and dump-tool capabilities. This compatibility check is
  not a substitute for quiesce, a successful dump, or a restore drill.
- Dump-tool discovery uses only reviewed `pg_dump --version` and `mysqldump
  --version` commands over pinned SSH, with a 64 KiB local output cap. It does
  not connect to a database or disclose credentials.
- Database connection metadata supports an `sshPeer` mode and a credential
  reference mode. `sshPeer` is restricted to `localhost` or `127.0.0.1`; it
  runs only fixed `psql --no-password` or `mysql --skip-password` server-version
  queries under the pinned SSH profile. The backup account must already have a
  non-interactive local database authorization path; otherwise the probe fails
  closed. Validation rejects unsafe identifier syntax before any adapter is
  called, and secret bytes never become remote command arguments. The
  credential-reference mode has no transport adapter yet.

### Embedded-database capture

- The only remote command is fixed (ADR 0005):
  `[ -f '<path>' ] || exit 1; tmp=$(mktemp) || exit 1; sqlite3 '<path>' ".backup '$tmp'" && zstd -q -c "$tmp"; status=$?; rm -f "$tmp"; exit $status`,
  where `<path>` is the one operator-configured, already-validated absolute
  file path, shell-quoted like every other remote command. The path existence
  check fails closed instead of letting `sqlite3` silently create a fresh
  empty database at a mistyped path. The remote temporary file is removed
  before the command exits on every path, success or failure.
- `sqlite3 .backup` is SQLite's own consistent-snapshot mechanism: it is safe
  to run against a live, concurrently written database, including one using
  WAL, without any application quiesce step.
- A narrow, fixed `command -v sqlite3` capability probe gates capture; no
  operator input reaches either the probe or the snapshot command beyond the
  one validated path. Unlike PostgreSQL/MySQL, no version-parity gate applies
  (SQLite has no client/server split).
- The captured stream is a single zstd-compressed file, not a tar archive. It
  is encrypted exactly like the filesystem payload before it can enter a
  sealed backup, and validated by a bounded zstd-stream inspector
  (`ZstdFileInspector`) that never writes the decompressed content to disk
  before restore. Restore decrypts and decompresses it directly to
  `database.sqlite`; there is no extraction/unpacking step because the
  payload is already exactly one file.
- Residual risk: the backup account needs read access to the configured
  database file and a working `sqlite3` binary on the remote host; capture
  fails closed if either is missing, but neither is otherwise verified.
- Before that snapshot command runs, a second fixed read-only probe
  (`stat -c%s` plus `df -Pk` on the same already-validated path) reports the
  database file's exact size and the containing filesystem's free space;
  capture fails closed if free space would not cover the file size plus a
  fixed margin. This exists because `.backup` writes a full uncompressed
  copy to a remote `mktemp` scratch file before compression — without this
  check, a large database on a nearly-full remote disk would only fail
  minutes into a capture, not immediately. The probe's two integers are
  parsed in Rust, never compared via remote shell arithmetic.

### Restore safety

- Default to dry-run and a new destination.
- The restore planner rejects unsealed manifests and relative targets, and
  requires an exact confirmation phrase before extraction.
- Verify backup signature/checksums immediately before mutation.
- Re-confirm server identity and show all deletions/service impacts.
- Create a safety point before destructive in-place restore.
- Database restore targets a new database/container first where practical.
- Hooks captured from the server are data, never automatically executable.
- A restore with both filesystem and database payloads stages both under one
  fresh sibling directory next to the destination, publishing them with a
  single atomic rename guarded by a fresh existence check immediately before
  it — a failed second payload can never leave a partial destination in place.

### Deploy safety

- Deploy targets a *different*, separately-enrolled, host-key-pinned profile
  than the one the backup was captured from — blocked on both a matching
  profile ID and a matching pinned host-key fingerprint (ADR 0007).
- The remote path must be currently absent. A filesystem-only deploy pushes
  and renames atomically in one step. A combined (filesystem-plus-database)
  deploy stages both payloads under one shared remote staging directory
  first — neither push renames individually — then a single finalize step
  atomically renames the staged directory into place, so an interrupted push
  never leaves a partially-written target and a failed second payload can
  never leave a live target with a missing database file.
- Each payload's manifest signature and checksums are re-verified
  immediately before that payload is pushed, not once for the whole
  operation, since each push is network-bound and can run for minutes.
- Before any filesystem payload is sent to a remote extractor, the decrypted
  tar.zst stream is inspected locally against the same path, entry-type, and
  resource limits used by capture and local restore; a rejected stream never
  opens remote deploy staging.
- Requires an exact confirmation phrase, computed from the backup ID, the
  target profile ID, and the target path together, before any push begins.
- Every deploy attempt is recorded to the repository's audit log at
  attempted/completed/failed states, keyed by a fresh identifier per
  invocation so retries never collide with or silently overwrite a prior
  attempt's record.

### UI and local API

- Tauri capabilities are allowlist-based. No generic shell or filesystem plugin
  is exposed to the WebView.
- Command DTOs are validated, job IDs are unguessable, and errors are redacted.
- The WebView renders native failure detail only when `code`, `message`, and
  `remediation` pass one bounded structural decoder. Unknown, malformed, or
  oversized rejection payloads use a fixed redacted fallback instead.
- Rich backup content is never rendered as raw HTML in the WebView.

### MCP server

- `guardian-mcp` (ADR 0012) uses stdio transport only — never streamable
  HTTP or any other network-reachable transport. A stdio pipe is only
  reachable by the direct parent/child process relationship, so this is a
  structural property, not a configuration choice that could be gotten
  wrong (unlike a loopback HTTP listener, which remains vulnerable to
  DNS-rebinding from any browser tab on the same machine).
  `execute_restore`/`execute_deploy` require the exact confirmation phrase a
  prior `preview_restore`/`preview_deploy` call returned, passed through to
  the same `RestorePlan`/`DeploymentPlan::approve` check every other surface
  already uses — the server never auto-fills or bypasses this field.
- Enrollment, credential import, agent-key registration, repository
  registration, vault initialization, signing enrollment, and capture-plan
  creation are not exposed as tools: each either mints new local trust or
  configuration state (a human judgment call, and a prompt-injection risk if
  an agent could be steered into enrolling a host key from untrusted
  content it read elsewhere) or, for capture-plan creation specifically, has
  no confirmation gate of its own to begin with.

## Key rotation

Server credentials and backup-node signing keys have independent identities.
Credential rotation does not rewrite backups. Signing-key rotation creates a
new trusted key record while retaining old public verification keys. Private
signing material is never exported with ordinary settings.

## Residual risks

- A fully compromised backup node can steal keys and alter new backups.
- A compromised privileged VDS account can present internally consistent but
  malicious data.
- Local read-only flags do not stop an administrator or ransomware with equal
  privileges.
- Backing up encrypted application data without its external keys may produce an
  unrecoverable but checksum-valid snapshot.
- Docker image tags are mutable; recovery plans should record digests and retain
  Compose/env material securely.
- Deploy's "target verified absent" check is a check against a remote
  filesystem outside Guardian's exclusive control, not a guarantee about the
  instant of the final atomic rename; the mitigation bounds the damage
  (never clobbers a value that raced into the path) rather than eliminating
  the race outright.
- If a deploy's remote host crashes before its own cleanup trap runs, an
  orphaned sibling temp path can be left behind next to the target — never
  a partial target itself, since the atomic rename is the only way content
  reaches the real path.

These risks are addressed operationally through independent nodes, least
privilege, offline/off-site copies, signed releases, and regular clean-room
restore drills.

## Release artifacts

Desktop installers are published only from protected `v*` tags after canonical
Windows/Linux verification and the Linux SSH/clean-room drills. Windows bundles
receive Authenticode signatures and Linux bundles receive detached OpenPGP
signatures; `SHA256SUMS` is generated after those platform signatures and is
itself OpenPGP-signed. The workflow fails closed when required signing secrets
are unavailable. Tauri auto-update endpoints and updater keys remain disabled;
the release workflow is a distribution control, not an update channel. GitHub
also records an OIDC/Sigstore provenance attestation for the final release
files, independent of installer and OpenPGP keys. See ADR 0014 and
`docs/RELEASE_SIGNING.md`.
