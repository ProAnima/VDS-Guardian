# Product Requirements — simple manual server backup

Status: accepted product contract for the operator experience. Implementation
status remains tracked only in `DEVELOPMENT_PLAN.md`; a requirement appearing
here does not mean it is already implemented.

## Product promise

VDS Guardian is a small manual backup application for remote Linux servers. A
non-technical operator must be able to add a server, browse what is on it,
select files or Docker-backed application groups, create an encrypted backup,
and understand a restore before anything is written. The normal path must not
expose signing identities, payload envelopes, capture-plan hashes, repository
internals, or other architecture terms.

The supported storage model is deliberately small:

- backups live in one local or removable filesystem repository;
- the desktop operator can rebind a registered repository to its existing
  folder after a drive-letter or mount-path change; removing a registration
  requires confirmation, never deletes backup files, and is blocked while a
  saved backup plan still references it;
- application metadata uses local JSON documents and SQLite where relational
  state is useful; no PostgreSQL/MySQL service is required by VDS Guardian;
- backup and restore are operator-triggered from desktop or through the typed
  MCP interface;
- scheduling, cloud/S3, replication, automatic retention, and an HTTP service
  are not required;
- SQLite on the protected server is the only database-specific capture adapter
  required for the simple path.

## Human navigation

### Servers

The Servers view contains only server management:

1. saved servers appear as readable cards with name, address, user,
   authentication kind, last verified state, and concise actions;
2. adding a server is one short form followed by a real pinned-host preflight;
3. supported authentication choices are SSH key/agent and login password;
4. deleting a server is a two-click, explicitly confirmed operation;
5. a server referenced by a saved backup selection cannot be deleted until the
   reference is removed or replaced.

Secrets never appear on cards, in portable settings, diagnostics, logs, or
repository metadata. Password authentication is a required capability but is
not considered implemented until the secure broker described by ADR 0015 and
the security tests in `SECURITY_MODEL.md` exist.

### Backups

The Backups view starts with a server picker and a visual, read-only explorer.
It combines two discoverable sources without pretending they are the same:

- a remote filesystem tree with folders, regular files, sizes, and bounded
  metadata;
- Docker containers and Compose/application groups with their capturable bind
  mounts and named volumes.

The operator selects folders, files, mounts, containers, or groups. The UI
shows the exact resolved host paths before capture. Selecting a container or
group never means “save the image”: it selects its capturable persistent paths.
Duplicate or nested roots are normalized before preview. Unreadable,
non-persistent, tmpfs, socket, device, symlink-target, and unsupported-volume
items are visible only with a clear non-selectable reason.

One primary action, **Create backup**, opens a concise preview containing:

- server and pinned identity;
- selected logical items and resolved paths;
- optional SQLite snapshot path;
- destination repository and estimated/known size where available;
- consistency warnings, especially live Docker volumes;
- the fact that capture is encrypted, verified, signed, and sealed.

Desktop may persist a validated selection for reuse. MCP may browse and run an
explicit selection only through preview then execute with the returned
confirmation phrase. Neither surface accepts an arbitrary remote command.

### Restore

Restore begins with a sealed, freshly verified backup. Its visual impact model
has three separate collections:

- `adds`: data written to a new destination;
- `replaces`: existing data that an approved future in-place mode would replace;
- `conflicts`: paths or workloads that prevent safe execution.

Restore is remote-first and offers a new destination or the managed source
replacement defined by ADR 0016. Replacement is available only for a backup
with signed source-layout metadata and requires a fresh safety backup, service
plan, typed confirmation, same-filesystem staging, health checks, and rollback.
The UI must describe the short service stop and never imply direct writes over
live data or zero downtime.
Docker labels in a backup are presentation metadata for impact explanation;
filesystem and SQLite payloads remain the recovery truth.

## Application contracts

Names below are semantic contracts. Rust domain types own validation; desktop
DTOs and MCP schemas serialize the same fields and must not invent alternate
rules.

```text
ServerConnectionSummary {
  profileId, label, host, port, user,
  authKind: "ssh_key" | "ssh_agent" | "password",
  verification: "verified" | "needs_recheck" | "unavailable"
}

RemoteBrowseRequest {
  profileId,
  directory,          // validated absolute remote path
  cursor?,            // opaque, response-bound cursor
  limit               // bounded by the use case
}

RemoteBrowseEntry {
  name, absolutePath,
  kind: "directory" | "regular_file" | "symlink" | "other",
  size?, modifiedAt?, selectable, unavailableReason?
}

RemoteBrowsePage { directory, entries, nextCursor?, truncated }

BackupSelectionItem =
  RemotePathSelection { absolutePath }
  | DockerMountSelection { containerId, mountDestination, capturablePath }
  | DockerGroupSelection { groupId, capturablePaths[] }

BackupSelection {
  profileId, repositoryId, items[], sqlitePath?
}

CaptureSelectionPreview {
  normalizedRoots[], logicalItems[], warnings[], confirmation
}

RestoreImpactPreview {
  backupId, targetProfileId, mode: "separate_path" | "replace_original",
  destination?, roots[], archiveEntries[], dockerWorkloads[],
  adds[], replaces[], conflicts[], confirmation, safetyBackupRequired
}
```

Contract rules:

- every list and string is bounded; pagination is mandatory for arbitrary
  directory listings;
- cursors are opaque, bind the offset to the sorted listing snapshot, and
  cannot smuggle a path or command; a changed listing invalidates the cursor;
- all remote names and metadata are untrusted UTF-8 input;
- symlinks are never followed by browsing or silently converted into capture
  roots;
- Docker selection is compiled into the same validated filesystem-root type
  used by ordinary selection;
- preview performs all normalization and authorization checks again at execute;
- MCP never auto-fills confirmation values and never enrolls server trust;
- errors are typed, redacted, and include safe remediation.

## Acceptance path

The target usability drill is successful only when a fresh operator can:

1. add one server from the Servers view without editing JSON;
2. open Backups, browse `/srv` and Docker mounts, and select data visually;
3. create and verify one filesystem-plus-optional-SQLite backup;
4. preview a restore and correctly identify what will and will not change;
5. restore to a new destination and preview a managed source replacement;
6. delete an unused server from its card;
7. perform the equivalent browse/preview/capture/restore path through MCP using
   explicit confirmations, without exposing a secret or arbitrary shell.
