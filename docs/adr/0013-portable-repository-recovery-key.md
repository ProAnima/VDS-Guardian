# ADR 0013: Portable repository recovery key

## Status

Accepted.

## Context

Release 0.1 section 2 (`docs/DEVELOPMENT_PLAN.md`) is the last unstarted
gate for the release: every payload's AES-256-GCM data key lives only in
the OS credential store, or, since ADR 0006, an operator-selected
`guardian-vault` file. Losing the operator machine loses every key, making
an otherwise-intact backup disk permanently unreadable — a disaster-recovery
tool whose own recovery keys cannot survive the loss of one machine.
ADR 0004's own Consequences section names this gap directly ("Cross-node
recovery and portable key export need a separate explicit wrapped-key
decision"), and `AGENTS.md`, `README.md`, and `docs/SECURITY_MODEL.md` all
flag it as a release blocker.

Gate stated in `docs/DEVELOPMENT_PLAN.md`: *"a clean machine can verify and
decrypt an existing backup using only the documented recovery material,
while a missing or incorrect recovery key fails closed."*

This design was produced by a grounded research pass (tracing every claim
against `staging.rs`, `manifest.rs`, `guardian_encryption`, and
`guardian-vault`'s canary mechanism) and a Plan-agent pressure test that
corrected two real mistakes in an earlier draft — a credential-ID scheme
that could overflow `CredentialId`'s length cap, and an underestimate of
the restore-side plumbing actually needed — before implementation began.
Two further real design gaps were found and fixed during implementation
itself (see "Corrections found during implementation" below).

## Decision 1 — one repository recovery key, envelope-wraps every payload key

One fresh random 256-bit key per repository (`guardian_encryption::PayloadKey`
— the same type already used for payload data keys and the vault's own
master key). It wraps every payload's data key as a *second*, additional
copy — the primary `SecretStore` entry (OS keyring or vault) is completely
unchanged, so routine capture/restore is byte-for-byte identical to today.

**Why one key per repository, not a passphrase per payload**: a per-payload
passphrase would mean typing or storing one at every single capture,
defeating the "backups are created unattended, by a machine, with no human
present" model this product already assumes (identical reasoning to why
ADR 0006 rejected a vault passphrase). The two-level wrap (repository key
wraps payload keys silently at every capture; a passphrase only wraps the
repository key, once, at deliberate export/import time) means a human is
involved exactly once per repository, only on the recovery path.

## Decision 2 — generation is explicit and capture-time-mandatory

`LocalRepository::configure_recovery_key(&self, secrets: &dyn SecretStore)`
(new): under the repository lock, fails closed as
`RepositoryError::RecoveryKeyAlreadyConfigured` if one is already set —
regenerating would silently orphan every payload already wrapped under the
old key, mirroring `guardian-vault`'s own master-key `write_new`, which
enforces the identical "never regenerate" rule. Otherwise it generates a
`PayloadKey`, mints a **randomly** minted credential ID
(`format!("recovery-{32-hex}")`, mirroring
`staging.rs::random_credential_id("payload")` — **not** a name derived from
`repository_id`, which the Plan-agent pressure test found could overflow
`CredentialId`'s 64-char cap for a legitimately-shaped, longer-than-today's
repository ID), and persists the credential ID as a **public** field on
`RepositoryMetadata` (`repository.json`, which already stores
`repository_id` this way).

**Capture fails closed without it, rather than silently skipping the
wrap**: `FilesystemCaptureComposition` already has this exact shape of
precondition — `require_preflight`/`require_disk_budget`, both checked
before any SSH byte streams. A new `require_recovery_key(&self)` joins them
in both `execute_filesystem_only`/`execute_combined`, returning a new
`CaptureUseCaseError::RecoveryKeyRequired`. `StagingBackup::
encrypt_and_register_payload_file` (`staging.rs`) *also* re-checks and
fails closed on its own (defense in depth, matching how
`PayloadEntry::validate()` re-validates even though every caller is
"supposed to" already pass valid values) — it must never silently produce
a payload with no recovery wrap. Making this silently optional-per-backup
would let an operator accumulate encrypted backups that specifically fail
the stated Gate, with no warning until the exact moment they lose the
original OS-keyring state.

New CLI: `guardian-cli recovery init --repositories-dir <dir>
--repository-id <id> --signing-config-dir <node-dir> [--vault-dir <dir>]
--json`, plus `recovery status`
(mirrors `vault status`) reporting only whether recovery is configured —
never the key itself.

## Decision 3 — the wrap lives in the manifest as one new optional field

`PayloadEncryption` (`crates/guardian-core/src/manifest.rs`) gains, as its
last field: `#[serde(skip_serializing_if = "Option::is_none")]
recovery_wrapped_key_base64: Option<String>`, plus a builder
`with_recovery_wrapped_key(&[u8])` (encodes internally, mirroring `new`'s
existing `nonce: &[u8; 12]` convention rather than taking a pre-encoded
string) and a `recovery_wrapped_key() -> Result<Option<Vec<u8>>,
ManifestError>` accessor (decodes internally, sparing every caller its own
base64 dependency just to read one field). **No `format_version`/
`envelope_version` bump** — this describes an orthogonal wrap of the *key*,
not the payload stream's own format; a v2 backup sealed before this feature
simply lacks the field, a permanent limitation named explicitly: sealed
manifests are immutable after signing, so old backups can never be
retroactively backfilled by editing their manifest.

`staging.rs::encrypt_and_register_payload_file`, right after the existing
unchanged `secrets.store(&credential_id, ...)` line: loads the repository
recovery key (fails closed per Decision 2 if absent), wraps the payload
key's raw 32 bytes via `guardian_encryption::encrypt_reader_to` over a
`Cursor`-wrapped buffer — the exact same self-describing-envelope idiom
`guardian-vault`'s own canary already uses, no new crypto primitive — with
its own domain-separated AAD binding `backup_id` + `payload_path`
(`recovery_wrap_associated_data`, symmetric to `staging::associated_data`,
distinct domain string `"guardian-recovery-wrap-v1"`).

`PayloadEncryption::validate()` gains a decode-and-length check when the
field is present: the self-describing envelope's output for a fixed
32-byte plaintext is deterministically 95 bytes (21-byte header + one
53-byte data frame + one 21-byte empty final frame) — this constant is
duplicated locally in `manifest.rs` (commented with its derivation) rather
than adding a `guardian-core` → `guardian-encryption` dependency edge,
matching ADR 0012's own precedent for small, well-commented cross-crate
duplication over a backwards dependency.

## Decision 4 — restore-side fallback

`execute_restore`/`open_deploy_payload_reader` each resolve the repository
recovery key **once** per call via a new `LocalRepository::
load_recovery_key(&self, secrets: &dyn SecretStore) -> Result<Option<
PayloadKey>, RepositoryError>` (crate-internal), then thread `recovery:
Option<&PayloadKey>` down through `stage_restore_payloads` →
`extract_payload`/`extract_database_payload` → `decrypted_payload_reader`.
Inside that function, a new `resolve_payload_key` helper: the primary
`SecretStore` entry is tried first; only if that lookup returns `None` does
it fall back to unwrapping `encryption.recovery_wrapped_key()` with the
resolved recovery key (`decrypt_self_describing_reader_to` with the
matching AAD). Either fallback input being unavailable collapses into the
same `RepositoryError::Credential` this already returned before recovery
wrapping existed — no new restore-side error variant, so the existing test
`encrypted_restore_fails_closed_when_the_key_is_missing` keeps asserting
exactly that.

`load_recovery_key` never acquires the repository lock itself (unlike the
public `recovery_credential_id()`/`configure_recovery_key`), specifically
so it is safe to call from contexts that already hold it (see "Corrections
found during implementation" below) — its own unavailability is never
treated as fatal by itself; only "neither primary nor fallback worked,"
decided entirely inside `resolve_payload_key`, is.

## Decision 5 — the recovery bundle: passphrase + Argon2id, a separate portable file

`guardian-cli recovery export --repositories-dir <dir> --repository-id <id>
--passphrase-file <path> --output <path> --confirmation "EXPORT RECOVERY
BUNDLE FOR <repository-id>" [--vault-dir <dir>] --json`: reads the
repository recovery key (fails closed if `recovery init` was never run),
reads the passphrase from `--passphrase-file` — a file, never a bare CLI
argument (avoids shell-history/process-list exposure) — raw bytes, UTF-8,
trimmed only of a trailing `\r`/`\n` run via `trim_end_matches(['\r',
'\n'])`, mirroring the exact existing idiom in
`guardian-ssh::secret_identity::classify_secret` rather than
`import-ssh-key`'s fully-opaque-bytes handling, since a passphrase is
uniquely likely to be hand-typed into a file and an editor's appended
trailing newline must not silently change the derived key.

Derives a wrapping key via Argon2id
(`crates/guardian-encryption/src/recovery_bundle.rs`; new workspace
dependency `argon2 = { version = "0.5", default-features = false, features
= ["alloc"] }` — confirmed against live `docs.rs` that the raw
`Argon2::new(algorithm, version, params).hash_password_into(passphrase,
salt, &mut out)` API needs no `password-hash`/`SaltString` machinery).
Parameters are pinned explicitly rather than trusting a crate-version
default — `KdfParams::recommended()`: `m_cost = 65536` (64 MiB), `t_cost =
3`, `p_cost = 4`, `output_len = 32` — OWASP's Argon2id baseline, chosen
generously since this runs once per deliberate export/import, never on a
routine path. Encrypts the repository recovery key under the derived key
(same `encrypt_reader_to`/`Cursor` idiom again), with AAD binding
the repository ID together with the active Ed25519 public verification key —
**required, not decorative**. This lets a clean machine verify sealed
manifests without exporting the private signing seed and prevents either the
repository identity or trusted verifier from being substituted. AEAD auth failure on a mismatch
makes this fail closed for free, the same way a wrong passphrase already
does — both collapse into one `RecoveryBundleError::
WrongPassphraseOrCorruptBundle`, deliberately not distinguished further,
since neither distinction is actionable differently by a caller.

Bundle file (JSON, matching every other metadata file in this codebase —
`repository.json`, `manifest.json`, `profiles.json`):
```json
{"formatVersion":1,"kdf":"argon2id","mCost":65536,"tCost":3,"pCost":4,
 "saltBase64":"...","ciphertextBase64":"...","verificationKey":
 {"algorithm":"Ed25519","keyId":"ed25519:...","publicKeyBase64":"..."}}
```
No separate nonce field — already embedded in the self-describing envelope.
`guardian_encryption::recovery_bundle` stays JSON-agnostic. The shared
`guardian-local-repository::recovery_bundle` service owns the JSON shape,
strict file-boundary validation, pinned KDF validation, bundle binding, and
typed confirmation gate; desktop and CLI call that service. The CLI retains
only its adapter-specific passphrase-file input policy.

`guardian-cli recovery import --repositories-dir <dir> --repository-id <id>
--repository-path <path> --input <path> --passphrase-file <path> --confirmation "IMPORT RECOVERY
BUNDLE FOR <repository-id>" [--vault-dir <dir>] --json`: the clean-machine-
restore side. When the clean registry has no entry, `--repository-path`
opens the transferred repository by its embedded ID and registers it locally.
It then derives the same unwrap key and decrypts (wrong passphrase or
wrong repository ID both fail closed via plain AEAD auth failure), and
stores the recovered key into the target machine's own configured
`SecretStore` via `LocalRepository::import_recovery_key` (see Decision 6)
— so every existing restore/deploy code path (Decision 4) works completely
unchanged afterward.

**Why not store the bundle inside the repository directory itself**:
defeats the purpose — the repository disk would again be a single point of
failure for the exact scenario this feature exists to close. The bundle's
value is being an independently-storable, offline artifact (USB drive,
safe, password-manager attachment); `docs/OPERATIONS_RUNBOOK.md` documents
this offline-copy procedure explicitly, separate from any future "export
settings" convenience feature (which must never embed this bundle or the
raw key).

## Decision 6 — confirmation gate: a typed phrase, matching restore/deploy

Both `recovery export` and `recovery import` require an explicit
`--confirmation "<VERB> RECOVERY BUNDLE FOR <repository-id>"` argument,
computed deterministically from public information (no separate preview
call needed, unlike restore/deploy where the phrase can depend on
otherwise-invisible plan details). This satisfies `docs/DEVELOPMENT_PLAN.md`'s
"confirmation-gated" as a quality distinct from merely "explicit," reusing
this codebase's own established idiom for "an operation whose blast radius
warrants a deliberate, typed acknowledgment" — decided explicitly with the
user rather than assumed, given the equally-plausible alternative of
reusing `credential.rs`'s plain write-once/explicit-invocation bar (the one
already applied to SSH private keys and the vault master key). `recovery
init`/`recovery status` do not need this (init is already write-once/
fail-closed; status is read-only).

## Corrections found during implementation

Two real design gaps surfaced only once real code was written and tested,
not during planning — named here rather than silently smoothed over, since
both are the kind of mistake worth remembering for future `guardian-core`/
`guardian-local-repository` work.

**Repository-lock reentrancy.** The original design had every recovery-key
method (`recovery_credential_id`, `load_recovery_key`, `configure_recovery_
key`) acquire the repository lock independently. This deadlocks the moment
any of them is called from a context that already holds it —
`StagingBackup::encrypt_and_register_payload_file` (which runs inside a
`begin_staging`-held lock for its entire lifetime) and
`open_deploy_payload_reader` (which holds its own lock for its whole
body) both do exactly this. The in-process lock registry this codebase
already uses specifically to work around same-process file-lock semantics
differing across platforms (`docs/ARCHITECTURE.md`) makes a second
acquisition from the same process fail with `RepositoryError::Busy` rather
than silently re-entering. Fixed by splitting each accessor into a
lock-free inner (`recovery_credential_id_locked`, and `load_recovery_key`
itself, which never acquires a lock) and a lock-acquiring public wrapper
(`recovery_credential_id`) used only by standalone callers (the CLI). This
is now the established pattern for any future `LocalRepository` method that
might be called from both a standalone context and from within
`StagingBackup`/an already-locked read path.

**`import_recovery_key` must reuse an already-recorded credential
reference, not reject it.** The original design routed `import_recovery_
key` through the identical "fail if `recovery_credential_id` is already
`Some`" logic as `configure_recovery_key`. This is wrong for the actual
clean-machine-restore scenario: the operator's clean machine has the *same*
repository directory (so the same `repository.json`, already recording the
original credential id from the machine that ran `recovery init`) together
with a fresh, different `SecretStore` — `recovery_credential_id` being
already `Some` is the *expected*, normal case for import, not an error
condition. Fixed by giving `import_recovery_key` its own logic: when a
credential reference is already recorded but absent from the active
`SecretStore`, import stores the recovered key under that same id. If a key
already exists, an identical import is idempotent and a different key is
rejected rather than overwriting working recovery material. Only a repository with no recorded reference at all mints
and records a new one, matching `configure_recovery_key`'s behavior for
that one case. Verified directly:
`import_recovery_key_reuses_an_already_recorded_credential_id` and
`import_recovery_key_never_overwrites_a_different_existing_secret`
(`guardian-local-repository/tests/local_repository.rs`) and the full CLI
round trip
(`init_export_import_recovers_byte_identical_key_material_on_a_fresh_secret_store`,
`guardian-cli/src/recovery.rs`).

## Decision 7 — placement, and what stays explicitly out of scope

- New `crates/guardian-encryption/src/recovery_bundle.rs`: the Argon2id +
  bundle envelope logic.
- Extended `crates/guardian-local-repository/src/repository.rs`
  (`RepositoryMetadata`, `configure_recovery_key`, `import_recovery_key`,
  `recovery_credential_id`, `export_recovery_key`, `load_recovery_key`, the
  `Option<&PayloadKey>` threading) and `src/staging.rs` (the wrap call) and
  `src/error.rs` (new `RecoveryKeyNotConfigured`/
  `RecoveryKeyAlreadyConfigured` variants — **not** reusing the existing
  `RecoveryRequired` variant, which already means something unrelated: an
  interrupted retention move needing manual inspection).
- New `crates/guardian-local-repository/src/recovery_bundle.rs`: the shared
  export/import use case, JSON contract, safe bundle file I/O, KDF policy,
  repository/signing binding, and confirmation phrases. `guardian-cli`
  remains the headless adapter for `init`/`status`/`export`/`import`.
- `crates/guardian-capture/src/lib.rs`: new `require_recovery_key`; new
  `CaptureUseCaseError::RecoveryKeyRequired` (`guardian-core/src/
  capture.rs`). `apps/desktop/src-tauri/src/job_commands.rs` and
  `crates/guardian-mcp/src/capture.rs` each gained one new match arm (both
  otherwise collapse every `CaptureUseCaseError` to one generic failure)
  with dedicated remediation text.
- No new crate — matches the stop-rule ("no new crate solely to hold one
  adapter... prefer a module").
- **Desktop repository recovery initialization is part of Release 0.1
  section 4** so a GUI-created repository can capture without a CLI detour.
  Desktop export/import must call the same shared recovery service; MCP
  remains excluded from recovery-bundle operations. This slice establishes
  the shared service and keeps the CLI as a thin headless adapter.
- `guardian-mcp` explicitly excludes any `recovery`-named tool from its
  surface, for the same reason ADR 0012 already excludes `vault init`/
  `register_repository`/`signing enroll` — one-time bootstrap/secret-
  bearing actions — and here specifically the single highest-blast-radius
  secret in the system. `excluded_tools_stay_excluded` asserts this.

## Alternatives rejected

- **A passphrase directly on every payload key, no repository-level
  indirection**: rejected for the same reason ADR 0006 rejected an optional
  vault passphrase — it does not fix the actual gap, since encrypted
  backups are created unattended with no human present to type one in. The
  two-level wrap keeps the passphrase needed exactly once per repository,
  only at deliberate export/import time.
- **Storing the recovery bundle inside the repository directory itself**:
  rejected — it would make the repository disk a single point of failure
  again for exactly the scenario this feature exists to close.
- **Making the recovery key optional/lazy per backup rather than
  capture-time-mandatory**: rejected — silently optional would let an
  operator accumulate encrypted backups that specifically fail the stated
  Gate, with no warning until the worst possible moment. Fail-closed-at-
  capture-time (with a defense-in-depth re-check at the lowest crate
  boundary) is the only shape consistent with this project's established
  "any ambiguity fails closed" sealing philosophy.
- **Deriving the repository recovery credential ID from `repository_id`
  (hash or string concatenation) instead of minting it randomly**:
  rejected — concatenation overflows `CredentialId`'s length cap for
  legitimately-shaped inputs (found by the Plan-agent pressure test before
  any code was written); every other credential ID in this codebase,
  including the vault's own secrets, is randomly minted, not derived.
- **A new dependency from `guardian-core` on `guardian-encryption`** just
  to validate the wrapped-key field's exact byte length: rejected — a small,
  well-commented duplicated constant is cheaper than a domain-layer crate
  depending on an infrastructure/crypto crate, matching ADR 0012's own
  precedent.

## Non-goals

Recovery-key rotation and recovery-bundle replacement (named explicitly in
`docs/DEVELOPMENT_PLAN.md` as Release 0.2 scope, not silently dropped here).
Desktop bundle export/import UI (Release 0.1 section 4) must use a shared
recovery service; repository recovery initialization is available from the
repository panel. Backfilling recovery wrapping onto backups sealed before this
feature shipped (impossible without violating "immutable after seal").
Passphrase strength enforcement or a passphrase-manager integration —
`--passphrase-file` accepts whatever the operator supplies; a weak
passphrase weakens the bundle's own protection, a risk the operator
controls and the ADR states plainly rather than attempting to police.

## Consequences

An encrypted backup repository can now be recovered on a clean operator
machine using only the sealed backups themselves, a documented recovery
bundle, and the correct passphrase — no dependency on the original
machine's OS credential store or vault state surviving. A missing or
incorrect recovery key fails closed at every layer (capture-time
precondition, restore-time AEAD authentication, bundle-level AEAD
authentication), matching the Gate this ADR exists to close.

Explicitly not delivered by this slice: key rotation, recovery-bundle
replacement, desktop bundle export/import, and retroactive wrapping of
pre-existing v2 backups. Follow-up on 2026-07-16: the clean-room restore drill
was extended to build the production CLI, remove the original operator state,
import the recovery bundle into a clean vault and registry, and restore through
that compiled CLI; the path subsequently passed on Linux CI. A later hardening
slice made clean-machine registration commit only after bundle authentication
and added compiled-CLI proof that wrong passphrases leave no registration,
while missing recovery keys and corrupted encrypted payloads leave no partial
restore target. The rest of the hostile-failure matrix remains release work.
