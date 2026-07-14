# Architecture

## System context

VDS Guardian runs on one or more operator-controlled backup nodes. Each node is
independent: it owns its schedules, credential references, audit log, and backup
repositories. Nodes do not trust or synchronize with one another in the initial
product.

```text
Windows or Linux operator
        |
Desktop UI or headless CLI/service
        |
guardian-core use cases
        |
  +-----+----------+-----------+------------+
  |                |           |            |
SSH adapter   storage adapter  keyring   scheduler
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

- SSH/SFTP transport with host-key pinning and keepalive/cancellation. The
  initial `guardian-ssh` adapter uses system OpenSSH through direct argv,
  temporary exact `known_hosts` input, and non-interactive strict host-key
  checking for read-only archive capture; it is not wired to a backup use case.
- Local repository with staging, atomic seal, read-only best-effort flags, and
  whole-directory retention.
- Secret storage backed by Windows Credential Manager and Linux Secret Service.
- Tar/Zstandard archive writer and hostile-input-safe reader. The initial
  `guardian-archive` adapter emits deterministic tar.zst streams and performs
  streaming inspection; extraction remains a later capability.
- Database adapters for PostgreSQL/MySQL and Docker-aware discovery/export.
- Native schedulers: systemd timer/service on Linux, Task Scheduler on Windows.

Implemented adapters are split into `guardian-local-repository`,
`guardian-signing`, `guardian-os-keyring`, `guardian-archive`, and
`guardian-ssh`. The signing crate depends only on
the core secret-store port; platform credential APIs remain isolated from domain
and repository code. Its application service serializes enrollment with a
cross-process lock and uses a durable intent to reconcile a keyring write that
completed before its public credential reference was committed.

### Desktop

React presents profiles, plans, job state, verification, and restore previews.
It calls typed Tauri commands through one bridge module. Tauri owns window and
OS integration only; blocking jobs run outside the UI thread and stream bounded
events. Signing status and explicit enrollment are the first infrastructure
commands: the Overview setup panel reads status, and only calls enrollment
after an explicit acknowledgement and final confirmation. Their Tauri functions
only resolve the app config path and dispatch the shared signing service to a
blocking worker.

### CLI/service

The CLI exposes the same use cases for automation. Signing status/enrollment
require JSON mode and an explicit absolute configuration path, and return
meaningful exit codes. Service installation is an explicit command and never
occurs simply by launching the desktop app.

## Backup lifecycle

```text
planned -> staging -> captured -> verifying -> sealed
               |          |           |
               +----------+-----------+-> failed/quarantined
```

1. Resolve a versioned backup plan and create a fresh staging directory.
2. Record pinned server identity, tool versions, plan digest, and start time.
3. Run preflight and capability probes with no mutation.
4. Capture database-consistent dumps and selected filesystem streams.
5. Record Docker Compose/config/image metadata without assuming images are
   sufficient to recover persistent data.
6. Finalize manifest and checksums; optionally scan under a strict resource cap.
7. Verify every payload and required plan item.
8. Sign manifest metadata using the backup node identity.
9. Atomically rename staging to its final backup ID and apply best-effort
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

Config updates use atomic write-and-rename and a schema version. Unknown future
fields are preserved where safe. Absolute local paths stay in ignored runtime
configuration, not committed fixtures.

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
- Jobs have cooperative cancellation; process trees and remote commands receive
  bounded shutdown before forced termination.
- Every external call has connect, idle, and total timeouts.
- Event queues are bounded so verbose remote output cannot exhaust memory.

## Compatibility

The manifest has independent format and producer versions. Readers support a
documented range and never "best effort" restore an unknown major format. Any
format migration produces a new sealed backup; it does not mutate the original.
