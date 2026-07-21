# ADR 0016: Guided source restore and managed cutover

Date: 2026-07-19

## Status

Accepted.

## Context

Release 0.1 exposes a local extraction form called Restore and a separate
remote new-host Deploy form. Operators reasonably interpret Restore as
restoring the protected VDS, but the current screen instead asks for a path on
the operator computer. A sealed backup also records only a profile reference
and payloads; Docker inventory is rediscovered for selection and then discarded.
Consequently the UI cannot truthfully show original paths, affected workloads,
or a safe in-place recovery plan.

Writing an archive over live paths is rejected. It can mix old and new files,
corrupt active databases, and offers no reliable rollback. The requested
"hot replacement" therefore needs a managed cutover, not an overwrite flag.

## Decision

New captures add an optional, signed `sourceLayout` to the manifest. It contains
the normalized source roots and the selected Docker workloads, including their
capture-time state, image identity, Compose project, and selected mount mapping.
It contains no environment values, secret values, commands, or credentials.
The same layout is retained with a saved capture selection and revalidated
before sealing. The field is optional so existing sealed backups and canonical
format-v1/v2 readers remain compatible.

Restore becomes remote-first and offers two explicit modes:

1. **Separate path** stages and publishes the verified payload under a new path
   on an enrolled, host-key-pinned VDS. The original source VDS is allowed when
   the destination is absent; this narrowly supersedes ADR 0007's blanket
   same-source rejection without weakening its no-clobber rule.
2. **Replace original** is available only when a verified manifest carries a
   valid source layout and the pinned source profile is enrolled. Preview
   rechecks current paths and Docker inventory and shows bounded archive entries,
   original roots, containers to stop/start, additions, replacements, and
   blocking conflicts.

Replacement execution requires the exact preview confirmation and performs:

1. a fresh sealed safety backup of the current roots;
2. verified extraction into same-filesystem sibling staging paths;
3. stopping only the explicitly previewed existing containers that were active;
4. renaming current roots to rollback paths and staged roots into place;
5. restarting the previously active containers and running bounded health checks;
6. automatic reverse renames and service restart if cutover or health checks fail;
7. a durable attempted/completed/rolled-back/failed audit trail containing the
   safety-backup ID.

The confirmation phrase is bound to a digest of the freshly inspected source
root and Docker impact. Execution repeats that inspection after the safety
backup; a changed image identity, container identity/name, Compose project,
mount mapping, missing root, or unwritable same-filesystem parent invalidates
the prior confirmation and blocks cutover. Operating-system roots such as `/`,
`/etc`, `/usr`, and `/boot` are never eligible for managed replacement.

Cancellation remains cooperative during verification, safety capture, and
staging. Once the bounded rename transaction begins it runs to completion or
remote rollback; cancelling the desktop job cannot terminate observation
between renames. The remote transaction traps termination signals, retries
workload readiness for a bounded interval, distinguishes a proven rollback
from an incomplete rollback, and records the former as `rolled_back`.

For multiple roots the preview must disclose that the sequence is transactional
with rollback but cannot be globally atomic across filesystems. A root without a
same-filesystem staging location is a blocking conflict. SQLite payloads use
their database-aware restore path and may not be merged into live storage as an
ordinary file.

The archive explorer is derived from the freshly signature-verified and
authenticated payload, is bounded and paginated, and never trusts presentation
metadata as payload truth. `sourceLayout` explains ownership and intended
targets; archive entries and payload hashes remain authoritative.

Docker recovery in this decision controls existing containers only. It does not
recreate deleted containers, restore environment variables or secrets, pull
images, or claim application-consistent recovery. A missing or changed container,
mount, image digest, or Compose mapping appears as a conflict requiring a new
preview or a separate-path restore.

The UI may call this action "Replace original data". It must not promise
zero-downtime or label direct writes as hot replacement; a short, visible service
stop is part of the approved plan.

## Consequences

- Existing backups without `sourceLayout` remain restorable to a separate path
  but cannot automatically replace original data.
- Local extraction remains an advanced export/recovery operation rather than the
  primary Restore screen.
- This supersedes ADR 0015's statement that in-place restore is only an
  unspecified future mode and moves the bounded managed-cutover slice into the
  active product path.
- Automatic container recreation, arbitrary hooks, live database file copying,
  and silent overwrite remain rejected.
