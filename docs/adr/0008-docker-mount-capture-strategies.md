# ADR 0008: Docker named-volume and bind-mount capture via existing inventory data

## Status

Accepted.

## Context

Milestone 3 names "named-volume and bind-mount capture strategies" as a P0
item. Today `guardian-docker`/`guardian-core` only produce a read-only
container/mount inventory (`DockerInventory`, `DockerMount`) — nothing turns
that inventory into a path the existing filesystem-capture mechanism
(`FilesystemCaptureRequest.roots: Vec<String>`, tar'd by
`RemoteCapturePlan::from_roots`) can act on. Inventory and capture were,
until this change, fully disconnected.

A bind mount's `source_reference` is already a validated absolute host
path — feeding it into `roots` needs nothing new. A named volume's
`source_reference` is its bare *name*, not a path, which looked like it
would need a new remote probe (e.g. `docker volume inspect <name>`) to
resolve. It does not: the `docker inspect` JSON this codebase already
fetches and parses (`guardian-docker/src/lib.rs`'s `InspectMount`) includes
a `Source` field for volume-type mounts too — Docker's standard behavior,
the resolved host directory (e.g.
`/var/lib/docker/volumes/myvol/_data` for the default `local` driver). The
existing parser already deserializes this field; `into_mount()` simply
discarded it for `Volume` kind, keeping only `Name`.

## Decision

`DockerMount` gains `host_path: Option<String>`, populated from the
already-fetched `Source` field for volume-type mounts when non-empty, and a
`capturable_path(&self) -> Option<&str>` method that is the one place
branching on mount kind: `Bind` → `source_reference` (already a path),
`Volume` → `host_path` (resolved separately, may be absent), `Tmpfs` →
`None`. Everything that already reads `source_reference` is unaffected —
its meaning per kind is unchanged; `host_path` is purely additive.

With `capturable_path()` in hand, both mount kinds reduce to "here is an
absolute host path" — exactly what an operator-typed `roots` entry already
is. No new capture use case, manifest payload kind, or restore logic. This
mirrors ADR 0005's own shape: reuse the existing filesystem-capture
mechanism rather than build a parallel one.

### Explicitly out of scope, not silently dropped

- **Non-`local` volume drivers.** A container's `Mounts[]` entry never
  reports the volume's *driver* — only `docker volume inspect <name>`
  does, a second remote round-trip per volume this change does not add.
  `host_path` is trusted at face value; for a driver where `Source` is
  absent or doesn't point to a plain readable directory, capture fails
  closed the same way it always has for any unreadable root — no
  proactive driver check, no silent success either.
- **Consistency/quiesce.** `DEVELOPMENT_PLAN.md` names "quiesce hooks...
  versioned application adapters" as aspirational Milestone-3 scope;
  nothing of the sort exists in this codebase. A raw `tar` of a live,
  actively-written volume is a point-in-time snapshot with no consistency
  guarantee beyond what `tar` gives a file that changes mid-read. ADR 0005
  solved this specifically for SQLite via `.backup`'s online-consistent-
  copy API; there is no generic equivalent for arbitrary volume data.
  Operators backing up a live database should prefer the embedded-database
  adapter (or a future dump-based adapter) over raw volume capture of that
  same data.
- **Privilege.** Reading a volume's or bind mount's host-side data needs
  filesystem access the backup account may not have by default (Docker's
  default volume storage is root-owned). This change requests, grants, or
  assumes no new privilege — no `sudo` added to the reviewed remote-command
  surface, no `docker` group requirement introduced. This matches ADR
  0002's existing model exactly: "a dedicated backup account with... a
  reviewed least-privilege sudo policy" is the operator's own, out-of-band
  responsibility, the same prerequisite already in place for the
  embedded-database adapter's "backup account needs read access to the
  configured database file." Capture fails closed on a permission error
  like any other unreadable root.
- **Discovery/selection.** Nothing yet turns "I have Docker inventory" into
  "these paths are now in my capture plan" — an operator (or a future UI)
  reads `capturable_path()` results and types them into `roots` themselves,
  same as any other root. A browsing/picker surface is real follow-up work,
  deliberately not bundled into this change (matching how ADR 0005 shipped
  its adapter before CLI/desktop UI followed in a later, separately
  approved slice).

## Consequences

- Both mount kinds named in Milestone 3 are now capturable through the
  existing, already-tested filesystem-capture path, with zero changes to
  restore or deploy.
- The `docker inspect` output this codebase already fetches is now used
  more completely — no new remote command, no new capability probe, no new
  dependency.
- The four items above remain real gaps against a fully "hands-off Docker
  backup" experience and should be tracked as their own follow-ups, not
  assumed solved by this change.
