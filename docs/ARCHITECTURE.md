# Architecture

## System context

VDS Guardian runs on an operator-controlled backup node. It owns credential
references, an audit log, and one or more backup repositories. Multiple nodes,
schedules, and synchronization are later operating capabilities and are not
part of the first-release correctness path.

```text
Windows or Linux operator
        |
Desktop UI or headless CLI/service
        |
guardian-core use cases
        |
  +-----+----------+-----------+
  |                |           |
SSH adapter   storage adapter  keyring
  |                |
Remote VDS      local/removable repository
```

## Workspace boundaries

### `guardian-core`

Owns the versioned domain model, backup lifecycle, policies, application ports,
and use cases. It may depend on small cross-platform libraries but not on Tauri,
a particular SSH client, or OS-specific APIs.

Planned modules:

```text
domain/       identifiers, plans, manifests, states, policies
app/          create, verify, list, restore-plan, restore, retain use cases
ports/        remote, storage, secret, clock, audit, scheduler abstractions
events/       bounded job events consumed by GUI and CLI
```

### Infrastructure crates

Adapters will be added by capability, not bundled into the domain crate:

- SSH/SFTP transport with host-key pinning and keepalive/cancellation. A
  read-only `RemoteBrowserPort` provides bounded, paginated directory pages
  without following symlinks or accepting arbitrary commands. The
  `guardian-ssh` adapter uses system OpenSSH through direct argv, temporary
  exact `known_hosts` input, and non-interactive strict host-key checking;
  it now covers read-only archive/database capture, Docker inventory
  inspection, and (ADR 0007) pushing a sealed backup's payloads onto a new
  host, and is wired into the full capture-to-seal use case by
  `guardian-capture`.
- Local repository with staging, atomic seal, read-only best-effort flags, and
  whole-directory retention. Staging can reserve an exclusive payload path for
  a streaming adapter, then registers the regular file and hashes it from disk
  before it can enter a manifest.
- Secret storage backed by Windows Credential Manager and Linux Secret Service,
  with an encrypted local file vault (`guardian-vault`) as an explicit,
  opt-in fallback for headless nodes without a usable OS credential store.
- Tar/Zstandard archive writer and hostile-input-safe reader. The
  `guardian-archive` adapter emits deterministic tar.zst streams, performs
  streaming inspection, and extracts only into a new directory after path,
  type, and resource-limit checks.
- An explicit application-consistent embedded-database snapshot adapter and
  Docker-aware discovery/export. PostgreSQL/MySQL server adapters are deferred
  from the initial product.
- Native schedulers are deferred until the manual Release 0.1 path is proven;
  later adapters may use systemd on Linux and Task Scheduler on Windows.

Implemented adapters are split by capability across `guardian-local-repository`
(staging/seal/retention), `guardian-signing` (Ed25519 node identity),
`guardian-os-keyring` (OS credential store) with `guardian-vault` as its
encrypted-file fallback, `guardian-archive` (tar.zst read/write), `guardian-ssh`
(pinned OpenSSH transport, above), `guardian-encryption` (the format-v2
payload envelope, ADR 0004), `guardian-database` (PostgreSQL/MySQL capability
discovery only — no dump/restore adapter yet), and `guardian-docker` (Docker
inventory inspection and mount-to-path resolution, ADR 0008).
`guardian-configuration` and `guardian-profile-store` hold the local,
non-secret configuration documents (repositories, capture plans, profiles).
Composition-root crates wire these adapters into full use cases:
`guardian-capture` (capture-to-seal, including the embedded-database
snapshot adapter, ADR 0005) and `guardian-deploy` (remote deploy to a new
host, ADR 0007). `guardian-cli`, the desktop's `src-tauri` crate, and
`guardian-mcp` (ADR 0012) are the three surfaces that call these composition
roots. `guardian-capture` reads repository free space through a small local
infrastructure port; production supplies the filesystem implementation and
the composition can deterministically prove its fail-closed disk budget.
The signing crate depends
only on the core secret-store port; platform credential APIs remain
isolated from domain and repository code. Its application service
serializes enrollment with a cross-process lock and uses a durable intent
to reconcile a keyring write that completed before its public credential
reference was committed.

### Desktop

React presents profiles, plans, job state, verification, and restore/deploy
previews. It calls typed Tauri commands through one bridge module (`shared/
commands.ts`). Tauri owns window and OS integration only; every blocking
command (enrollment, capture, restore, deploy, Docker inventory) runs on a
`spawn_blocking` worker outside the UI thread as a single request/response
call — there is no progress-event stream to the frontend yet, so a
long-running job's UI stays in a single loading state until it resolves.
Signing status and explicit enrollment were the first infrastructure
commands: the Overview setup panel reads status, and only calls enrollment
after an explicit acknowledgement and final confirmation. Their Tauri functions
only resolve the app config path and dispatch the shared signing service to a
blocking worker. SSH profile enrollment, repository registration, recovery-bundle
export/import, capture-plan save/run, Docker inventory browsing, and
restore/deploy preview-and-execute now follow the same shape. Recovery-bundle
commands call the shared `guardian-local-repository` service; desktop keeps an
entered passphrase in memory, requires it twice for export, and bundle import
authenticates before registering an unknown repository. Guided setup refreshes
dependent selectors after each completed step and excludes repositories whose
recovery key is unavailable from capture. SSH enrollment is one shared-core
transaction: it stages the credential, runs the pinned capture-capability
probe, commits the profile only on success, and removes the staged credential
after any failed probe or profile commit. The desktop server manager also owns
the explicit deletion adapter: it blocks profiles referenced by capture plans,
atomically removes an unused profile, deletes its OS-stored credential, and
compensates by restoring the profile if credential cleanup fails.
`guardian-mcp` remains excluded.
The desktop navigation separates the server manager from backup preparation:
the Servers view contains only saved server cards and enrollment, while the
Backups view owns signing, repository recovery readiness, and capture-plan
selection. This keeps infrastructure prerequisites out of the routine server
management path without duplicating their application services.
The Backups view composes `RemoteBrowserPort` with Docker inventory. The shared
`preview_capture_selection` policy owns the serialized logical-item contract,
re-resolves Docker mounts/groups against current inventory, removes duplicate
or nested roots, and returns warnings plus a deterministic preview identity.
Desktop consumes that preview before persisting the normalized roots. Making
the preview an execution precondition for desktop and MCP remains the next
application-service step.
The desktop explorer presents the two discovery sources as separate bounded
sections: a paginated filesystem table with path breadcrumbs, metadata,
non-selectable reasons, retry/loading/empty states, and current-folder
selection; and a Docker inventory grouped into Compose applications and
individual persistent mounts. A persistent selection summary preserves the
logical item identity rather than flattening Docker choices into UI-only path
strings. This is presentation state only; core preview remains authoritative.

### CLI/service

The CLI (`guardian-cli`) exposes some of the same use cases for automation
today: `profile`, `credential`, `restore`, `vault`, `deploy`, and `signing`
subcommands, each requiring JSON mode and an explicit absolute configuration
path, returning meaningful exit codes. There is no `capture`/`plan`/`job`
subcommand and none is planned — the desktop app is the sole first-class
human interface, and `guardian-cli` deliberately keeps its
enrollment/restore/deploy scope rather than growing one. Native scheduler
(systemd timer/Task Scheduler) integration and a service-install command are
both still design intent, not implemented; when added, service installation
should remain an explicit command, never an implicit side effect of
launching the desktop app.

### MCP server

`guardian-mcp` (ADR 0012) exposes discovery (profiles, repositories, capture
plans, Docker inventory, sealed backups), capture, restore, and deploy —
including the one operation `guardian-cli` deliberately excludes,
capture — as MCP tools for external tools and AI agents. Stdio transport
only: it runs as a local subprocess an MCP client launches directly, so it
inherits the same OS-process trust boundary every other surface already has
rather than creating a new, network-reachable one. `preview_restore`/
`preview_deploy` return the same confirmation phrase `RestorePlan`/
`DeploymentPlan::approve` already require; `execute_restore`/`execute_deploy`
require it back unchanged — the calling agent supplies it explicitly, the
same as a human copying it from a CLI or desktop preview. Enrollment,
credential import, repository registration, vault init, signing enrollment,
recovery-key init/export/import (ADR 0013), and capture-plan creation are
deliberately not exposed: each either mints new local trust/config state,
carries the single highest-blast-radius secret in the system, or (for
capture-plan creation specifically) has no confirmation gate of its own to
begin with.

## Backup lifecycle

```text
planned -> staging -> captured -> verifying -> sealed
               |          |           |
               +----------+-----------+-> failed/quarantined
```

1. Resolve a versioned backup plan and create a fresh staging directory.
2. Record pinned server identity, tool versions, plan digest, and start time.
3. Run preflight and capability probes with no mutation.
4. Resolve the operator's visual filesystem/Docker selection to explicit,
   normalized capture roots.
5. Capture database-consistent dumps and selected filesystem streams.
6. Record logical Docker selection metadata without assuming images are
   sufficient to recover persistent data.
7. Finalize manifest and checksums; optionally scan under a strict resource cap.
8. Verify every payload and required plan item.
9. Sign manifest metadata using the backup node identity.
10. Atomically rename staging to its final backup ID and apply best-effort
   read-only attributes. Only then is state `sealed`.

An interrupted staging directory is never resumed as a trusted backup. It may
be inspected or deleted after a grace period.

## Restore lifecycle

Restore is a separate use case, not "backup in reverse":

1. Verify format support, signature, all checksums, and required payloads.
2. Produce a deterministic dry-run plan with target, deletions, writes, service
   stops/starts, database operations, and expected downtime.
3. Require target identity verification and typed operator confirmation.
4. Create a fresh safety backup unless a break-glass waiver is recorded.
5. Restore into new paths/volumes where possible, validate, then switch over.
6. Run health probes and emit a signed restore report.
7. Preserve the previous deployment until rollback expiry.

The initial restore slice accepts only a sealed manifest and an absolute new
target path. It produces an exact confirmation phrase, re-verifies signature
and payload digest at execution, and then stages every present payload (the
required filesystem archive and, when the manifest carries one, the database
snapshot) into a fresh sibling directory, publishing all of it to the
destination with a single atomic rename only after every payload has
extracted successfully. Encrypted format-v2 payloads resolve their key
through the secret-store port and are authenticated before extraction;
since ADR 0013, if the primary secret-store entry is unavailable, restore
falls back to unwrapping the manifest's own recovery-wrapped copy of the
same key using the repository's recovery key, when one has been imported
into that secret store. Safety backup, switch-over, rollback, health
probes, and signed restore reports remain separate gates.

The local-repository adapter reloads and verifies the manifest signature and
every payload checksum immediately before it produces this plan. It rejects an
already existing target path, so a plan cannot be used to merge into live data.

## Configuration model

Configuration contains public profile data and secret references only:

```text
profile ID, display name, host, port, user
pinned host key fingerprint
credential reference (never secret bytes)
backup plan ID and schedule
repository ID and local path
retention and verification policies
```

Config updates use atomic write-and-rename and a schema version. Public
configuration documents preserve unknown future top-level fields where safe;
their validated security-bearing entries remain strict. Absolute local paths
stay in ignored runtime configuration, not committed fixtures.

Signing identity metadata is stricter than general profile configuration:
unknown fields fail closed. `signing.json` stores only a credential reference,
algorithm, and public key ID; an interrupted `signing-enrollment.json` is a
recovery journal, never private key material.

## Concurrency and cancellation

- One mutating job per server profile.
- Repository-level locks prevent two processes from sealing or retaining at the
  same time.
- Repository and signing locks combine an in-process path registry with an OS
  file lock because same-process file-lock semantics differ across platforms.
- Retention writes a durable non-secret intent outside its temporary quarantine
  directory. Opening a repository reconciles interrupted moves by rollback or
  resumes a durably marked cleanup; contradictory state fails closed.
- Operator-triggered cancellation (ADR 0010) covers SSH-backed capture and
  deploy plus desktop local restore. The CLI installs a Ctrl+C handler for
  deploy, the desktop app tracks all three operations in a per-run registry
  with a Cancel affordance, and `guardian-mcp` (ADR 0012) exposes the same
  registry for capture and deploy through a `cancel_job` tool. SSH transports
  poll the cross-thread handle between reads; local restore checks it while
  decrypting, decompressing, and extracting each payload and again before the
  final atomic publish.
  The `JobRegistry` desktop and `guardian-mcp` share lives in `guardian-core`,
  not duplicated per surface. The spawned child is isolated into its own
  process group so only that cooperative signal, never a raw OS interrupt
  racing it, ends the process. A cancelled local restore removes its fresh
  staging tree and never publishes the destination. The clean-room drill
  exercises capture and deploy cancellation after each stream transfers data;
  focused archive/repository tests cover local restore cancellation and cleanup.
- Every external call has connect, idle, and total timeouts.
- Capture and push streams enforce a maximum byte ceiling so a runaway or
  hostile remote command cannot exhaust local disk or memory.

## Compatibility

The manifest has independent format and producer versions. Readers support a
documented range and never "best effort" restore an unknown major format. Any
format migration produces a new sealed backup; it does not mutate the original.
