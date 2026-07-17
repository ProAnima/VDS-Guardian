# 0015: Simple server explorer and manual selection are the primary product path

Date: 2026-07-17

## Status

Accepted.

## Context

The existing Release 0.1 implementation proved difficult backup invariants but
presented them as a four-step setup wizard. Operators saw signing identity,
repository recovery, SSH enrollment, and raw path entry together. Existing
profiles were rendered as small pills and could not be deleted. Docker
inventory existed only as optional assistance. This was technically explicit
but failed the product's required “simple” category.

The accepted operator path is now server-centric: manage servers as cards,
browse one selected server, select visible filesystem or Docker-backed data,
create a backup, and review visual restore impact. The application remains
manual and local-first; SQLite is sufficient and no cloud/database/scheduler
platform is required.

## Decision

`docs/PRODUCT_REQUIREMENTS.md` becomes the product-experience contract. The
desktop navigation separates Servers, Backups, Restore, and Deploy. Setup
prerequisites may appear contextually in Backups when missing, but they do not
re-enter the Servers view or become the vocabulary of the normal path.

A new read-only remote-browsing application port returns bounded typed directory
pages. The transport adapter may use SFTP or a separately reviewed fixed remote
command, but it may not accept arbitrary shell text. Symlinks are described but
not followed. Docker inventory is projected into the same explorer as logical
container/group/mount nodes. Selection compiles to existing validated absolute
capture roots; the filesystem and optional SQLite payloads remain recovery
truth.

Capture selection becomes one shared DTO across desktop and MCP. MCP may gain
read-only browse and preview-selection tools plus an execute-selection tool
only when execute requires the preview's explicit confirmation phrase. This
partially supersedes ADR 0012's blanket exclusion of capture-plan creation:
MCP still cannot enroll trust or store credentials, but an explicit,
confirmation-gated capture selection is allowed. No HTTP listener is added.

Restore gains a presentation-level impact contract (`adds`, `replaces`, and
`conflicts`). This does not authorize in-place mutation. Release 0.1 remains
new-destination-only, so `replaces` is empty by construction. Any later in-place
mode still needs its own safety-backup and rollback implementation.

Server authentication becomes a tagged capability: SSH key, SSH agent, or
password. Host-key pinning is identical in every mode. Password support must
use a native SSH implementation or a one-shot askpass broker whose secret is
delivered through memory-only local IPC. Passwords are forbidden in argv,
environment variables, shell text, repository/config documents, logs, and
temporary files. `sshpass`, interactive terminal scraping, and disabling
`BatchMode` without such a broker are rejected alternatives. Until this adapter
and its adversarial tests exist, the UI must not advertise password mode as
available.

## Consequences

- Docker browse/mount selection moves from Release 0.3 candidate to the current
  operator-path gate, superseding that scheduling statement in
  `DEVELOPMENT_PLAN.md` and the “future UI” consequence in ADR 0008.
- Automatic Docker backup, container recreation, image export, quiescing, and
  live-database volume capture do not become implied features.
- The backup format may add signed logical-selection labels/mappings for visual
  restore explanation, but payload verification never depends on those labels.
- Password authentication is a real security-boundary project, not a form-field
  change. Key enrollment remains the first implemented mode.
- Product acceptance now includes desktop and MCP browse/selection flows, while
  the CLI remains intentionally without capture.

## Rejected alternatives

- Keep raw path textareas as the primary interface: technically direct but not
  understandable enough for the target operator.
- Treat a Docker container or image as a complete backup: it omits persistent
  state and creates a false recovery promise.
- Recursively scan the entire server at enrollment: excessive privilege,
  latency, disclosure, and denial-of-service risk.
- Store a login password in a profile JSON document or pass it through an SSH
  process environment/command line: secrets leak through ordinary OS and
  diagnostic surfaces.
- Enable in-place restore merely because a preview can display replacements:
  visualization is not a rollback mechanism.

