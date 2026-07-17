# 0012: MCP server for headless and agent access

Date: 2026-07-16

## Status

Accepted.

Amended by ADR 0015: read-only remote-directory browsing is now exposed, and a
future explicit capture selection may be accepted only through a preview plus
confirmation gate. Trust enrollment and credential handling remain excluded.

## Context

Earlier the same session, the product's interface philosophy was redirected:
the desktop app is the sole first-class *human* interface; `guardian-cli`
keeps its existing enrollment/restore/deploy scope and will not grow a
capture command (`docs/DEVELOPMENT_PLAN.md`'s product-boundary list and
section 3 were updated to reflect this, commit `ff4be87`). Headless and
programmatic access — explicitly including AI agents such as Claude Code
itself — is instead served by a typed external API: an MCP (Model Context
Protocol) server, `guardian-mcp`.

This is a brand-new, first-ever inbound interface this product exposes.
`AGENTS.md`'s security-review triggers explicitly include "SSH trust,
privileges, command execution, or remote scripts" and "restore planning or
remote mutation" — MCP tool calls become a new caller of all of these, so
this decision gets the same rigor as ADR 0007/0009/0010.

### Grounding, confirmed against the real code before designing anything

- **No unified "application-service boundary" existed** before this slice,
  despite `DEVELOPMENT_PLAN.md` section 3 naming one as a goal. CLI and
  desktop both call the same lower composition roots
  (`DeploymentComposition`, `FilesystemCaptureComposition`,
  `LocalRepository`), but each independently resolves storage roots
  differently, defines its own DTOs (two separate `DeployFailure` types with
  the same shape), hand-rolls the confirmation gate, and resolves secrets
  differently (desktop always uses `OsCredentialStore`; CLI supports
  `--vault-dir`).
- **`guardian-cli` has zero dependency on `guardian-capture`** — capture was,
  and remains, desktop-only among the two pre-existing surfaces.
- **`apps/desktop/src-tauri/src/job_registry.rs` had zero Tauri-specific
  content** — only `guardian_core::{CancellationHandle, RunId}` plus
  `std::collections::HashMap`/`std::sync::Mutex`. Relocated into
  `guardian-core` (this slice) so desktop and `guardian-mcp` share one real
  implementation instead of near-identical copies.
- The official Rust SDK for MCP is `rmcp` (`modelcontextprotocol/rust-sdk`,
  pinned to `0.16`), actively maintained, supporting stdio and
  streamable-HTTP transports, tool definition via `#[tool]`/`#[tool_router]`/
  `#[tool_handler]` macros with `schemars`-generated JSON schemas.
  Authentication beyond an optional OAuth2 feature is left to the
  implementer.

## Decision 1 — crate placement: `crates/guardian-mcp`, a new sibling crate

Not a `guardian-cli` subcommand: capture must be reachable through MCP, and
folding MCP into `guardian-cli` would pull `guardian-capture` into
`guardian-cli`'s dependency graph even if gated behind a different verb — a
boundary violation in substance, trivially checkable later by diffing
`Cargo.lock` if this rule is ever second-guessed. Dependencies are a
superset of `guardian-cli`'s own list plus `guardian-capture` (the one thing
CLI deliberately excludes), `guardian-docker` (Docker inventory),
`guardian-archive` (`ArchiveLimits`, mirroring desktop's capture wiring),
and the new external deps `rmcp`, `tokio`, and `rmcp`'s own re-exported
`schemars` (see the pitfall noted below). This is the first workspace crate
to depend on bare `tokio` directly — Tauri bundles its own runtime
internally, so this is a new, deliberate, expected addition, not a smell.

Module layout mirrors `guardian-cli`'s domain split (`capture`, `restore`,
`deploy`, `discovery`, `config`, `secret_store`). Every `#[tool]`-annotated
method in `lib.rs` stays exactly as thin as `guardian-cli`'s own command
functions: validate arguments, call one function in the matching domain
module, map one typed result. The domain modules hold the actual logic and
are unit-tested directly against their own public (`pub(crate)`) functions;
`lib.rs` only wires them to the MCP protocol.

**A `schemars` version pitfall worth naming explicitly**: `rmcp` 0.16
depends on `schemars` 1.x internally and re-exports it as `rmcp::schemars`
specifically so consumers derive `JsonSchema` against the *same* version its
own macros expect. Adding a separate, directly-versioned `schemars`
dependency (as originally attempted) silently pulls in a second,
incompatible major version (0.8.x) with the same crate name — every
`#[derive(schemars::JsonSchema)]` then fails to satisfy the trait bound
`#[tool]`'s generated code requires, with an error that does not obviously
point at "two versions of the same crate." Fixed by removing the standalone
`schemars` dependency entirely and importing `rmcp::schemars` instead.

## Decision 2 — one relocation now, one deliberately rejected

**Relocated**: `JobRegistry`/`JobRegistration` into `guardian-core`
(confirmed zero-risk per the grounding above). Desktop's `lib.rs`/
`job_commands.rs`/`deploy_commands.rs` updated to import it from
`guardian_core` instead of a local module; its 3 existing unit tests moved
unchanged.

**Deliberately not done**: relocating `guardian-cli/src/secret_store.rs`'s
`ResolvedStore`/`resolve_store` into `guardian-vault`. Checking
`guardian-vault/Cargo.toml` shows it does not depend on `guardian-os-keyring`
today — adding that dependency merely to host a ~30-line selector enum would
make a crate whose whole purpose is "the fallback for headless nodes without
a usable OS credential store" depend on the very store it is a fallback for,
a backwards coupling for a small convenience. Instead, this same enum and
resolver function are duplicated into `guardian-mcp`'s own `secret_store.rs`
— matching this project's own established tolerance for small cross-crate
duplication over a backwards dependency edge (the `TestSigner` duplication
between `guardian-local-repository` and `guardian-deploy`'s own tests; the
icacls/whoami duplication flagged as "not urgent" rather than unified).

## Decision 3 — transport and trust model: stdio only

**Stdio transport only** — no streamable HTTP, no other network-reachable
transport, for this and every planned future version unless a separate,
dedicated ADR revisits it. Reasoning, independently derived rather than
accepted at face value:

- **Structural, not configured, confinement.** A stdio pipe is only
  reachable by the direct parent/child process relationship — there is no
  way to misconfigure this into wider exposure. A loopback HTTP listener is
  a *policy choice* that can be gotten wrong (a bind-address typo), and
  loopback HTTP servers are the textbook target of DNS-rebinding attacks
  from any browser tab on the same machine — a risk class stdio is
  categorically immune to, not merely mitigated against.
- **No auth-model gap to fill.** There is zero user-account/session/token
  concept anywhere in this codebase today. `rmcp`'s HTTP-transport auth is
  "largely left to the implementer" beyond an optional, non-trivial OAuth2
  flag — building that for a server that can trigger remote deploys to
  arbitrary enrolled VDS targets would need its own ADR-level design (token
  issuance, rotation, revocation, storage, theft response), for a use case
  (a local coding agent) that does not need it.
- **Matches every existing surface's trust model exactly.** CLI and the
  desktop app are both local processes running as the operator's own OS
  user; this product has never had an inbound network listener (SSH is
  always outbound, initiated by this product toward a remote VDS).
- **Multi-client concerns don't actually favor HTTP.** `LocalRepository`
  already uses OS-level file locks specifically because independent
  processes can race on the same repository — that mechanism already
  covers two independent `guardian-mcp` subprocesses the same way it
  already covers `guardian-cli` and the desktop app running concurrently
  today. The only thing that doesn't survive across independent
  `guardian-mcp` subprocesses is the in-process `JobRegistry` — already just
  as true today between CLI and desktop (a CLI deploy can't be cancelled
  from the desktop's Cancel button either).

**Trust-model statement**: `guardian-mcp` runs with the same OS-user
privilege as whoever's MCP client (Claude Code, Claude Desktop, or any other
local tool) launched it, identical to today's rule for CLI and desktop. This
decision does not widen who can reach capture/restore/deploy beyond "a local
process the operator's own session spawned." A future genuine
multi-operator/remote-access need would warrant its own dedicated ADR with a
real authentication design, not a retrofit onto this one.

## Decision 4 — tool surface

**Read-only/discovery** (no mutation, no confirmation gate needed):
`list_ssh_profiles`, `list_repositories`, `list_capture_plans`,
`list_docker_containers`, `browse_remote_directory`, `list_backups` — each a thin wrapper over
`ProfileStore::list`/`RepositoryStore::list`/`CapturePlanStore::list`/
`DiscoverDockerInventoryUseCase`/`LocalRepository::list_sealed_backups`,
mirroring the exact desktop/CLI call shape.

**Plan/preview** (no mutation): `plan_capture` (new, but trivial — resolves
a saved plan's profile and repository into a preview DTO, no new domain
logic); `preview_restore` → `LocalRepository::plan_restore`; `preview_deploy`
→ `DeploymentComposition::plan`.

**Execute** (mutating): `run_capture` → `FilesystemCaptureComposition::
execute`, cancellable; `execute_restore` → `LocalRepository::execute_restore`,
**not** cancellable (matches the pre-existing gap — no SSH child, ADR 0010
explicitly deferred restore cancellation, not closed here either);
`execute_deploy` → `DeploymentComposition::execute`, cancellable.

**Control**: `cancel_job` → the relocated `JobRegistry::cancel`.

**Explicitly excluded from v1, with reasons, and enforced by a test** (see
Decision 6): `credential import-ssh-key`/`register-agent-key` (raw key
material); `profile enroll` (establishes a new pinned trust relationship;
`SECURITY_MODEL.md` requires explicit out-of-band host-key verification, a
human judgment call an agent should not mint unsupervised — also a real
prompt-injection surface if content read elsewhere suggested a host key);
`register_repository`, `vault init`, `signing enroll` (one-time bootstrap
actions creating new local state); `save_capture_plan` (decides which
absolute remote paths get captured and has **no confirmation gate of its
own today** — exposing it would make it the only MCP-reachable mutation
with zero human-in-the-loop check); a standalone `verify_backup` tool (no
such action exists in CLI or desktop today — verification is already
mandatory and inline inside every read/plan/execute path; a safe, cheap
follow-up if wanted, not part of this slice).

## Decision 5 — confirmation-phrase gates are never bypassed

`RestorePlan::approve`/`DeploymentPlan::approve` require the caller to
supply back a deterministic phrase computed from the operation's own
identifying fields — the same two-call preview-then-execute shape CLI and
desktop already both use. `preview_restore`/`preview_deploy` return the
`confirmation` field; `execute_restore`/`execute_deploy` require it as a
real input argument, passed straight through to the existing
`plan.approve(confirmation)` call with **no change to that logic**.

**Hard rule, never to be violated**: `guardian-mcp` must never auto-fill
this field from a prior tool call's result, and must never grow an
`auto_confirm: true`-style shortcut. The calling agent must supply it
explicitly every time, standing in for the human who would otherwise type
or paste it. `run_capture` has no equivalent gate to preserve (none exists
for capture anywhere today — desktop's "Run" button is the confirmation);
no new gate was invented only for MCP, matching `CODEX.md`'s "CLI and GUI
share... policy" principle. `plan_capture` is a preview-only convenience,
not a hard precondition for `run_capture`.

## Decision 6 — defer the full unified application-service boundary

`DEVELOPMENT_PLAN.md` section 3's actual Gate is behavioral parity ("the
same fixture plan produces the same sealed backup and restore result..."),
not a mandate for one literal shared crate. Since the layer that determines
the sealed backup's actual bytes/manifest is already properly shared, that
Gate is achievable with `guardian-mcp` as a third thin wrapper — provided
its tool handlers stay exactly as thin as CLI's/Tauri's own command
functions. A full unified boundary crate would be a bigger, separately
scoped refactor of the two most security-reviewed call paths in the
project, for a Gate that does not require it — deferred explicitly (named,
not silently dropped, matching this project's established amendment
pattern) as real future work if a fourth caller or genuine drift ever makes
the case.

An automated test enforces the "no accidental scope creep" half of this:
`excluded_tools_stay_excluded` asserts the tool router's registered tool
names never contain any enrollment/credential-import/vault-init/
signing-enroll/save-capture-plan substring, so a future addition to that
list fails a test rather than silently expanding what an agent can reach.

## Decision 7 — secret-store resolution follows CLI's pattern, not desktop's

Desktop always uses `OsCredentialStore` because it "always runs in a real
user session" (ADR 0006). `guardian-mcp` has no such guarantee — it is
plausibly the *most* likely-headless surface in the product (a coding agent
commonly runs on a remote Linux box/container with no desktop session bus,
exactly `guardian-vault`'s target scenario). `guardian-mcp` therefore
follows `guardian-cli`'s pattern: an optional `--vault-dir` startup
argument, consistent with its explicit-absolute-path-argument style
generally (there is no Tauri `app_config_dir()` equivalent to lean on),
hard failure if given but unopenable, never a silent OS-store fallback.

Unlike `guardian-cli` (which re-parses its directory arguments on every
short-lived invocation), `guardian-mcp` is long-lived — one process serves
many tool calls over its stdio lifetime — so `--repositories-dir`/
`--profiles-dir`/`--plans-dir`/`--config-dir`/`--vault-dir` are parsed once
at process startup (`ServerConfig::parse`) and held for the process's
lifetime, not re-supplied per tool call.

## Decision 8 — cancellation wiring

Capture and deploy are SSH-backed and fit ADR 0010's existing scope; restore
stays explicitly out of scope (named, not silently dropped). Because a tool
call is synchronous (blocks until the operation finishes, same as desktop's
blocking command pattern), `guardian-mcp` follows desktop's convention —
**caller-supplied `run_id`** (validated via `RunId::parse`), not CLI's
self-minted one (CLI has no concurrent second-call cancellation path; it
uses OS Ctrl+C instead, which has no MCP equivalent). `guardian-mcp`
registers a `CancellationHandle` under the given `run_id` in its own
in-process `JobRegistry` *before* the composition call starts (mirroring
desktop's exact ordering); `cancel_job` looks it up.

**A real tokio gotcha worth naming**: the composition roots here are all
synchronous, so a tool handler running one blocks the async runtime's
worker thread for the duration. Aborting an *async* task wrapping that call
would not stop the underlying blocking work — exactly why ADR 0010 built a
poll-loop-checked `CancellationHandle` for desktop's identical pattern
instead of trusting implicit async cancellation. The explicit `cancel_job`
tool plus `JobRegistry` is the real mechanism; whether MCP's standard
`notifications/cancelled` message could usefully trigger the same handle as
a bonus is left for future investigation, not required for this slice.

## Decision 9 — test strategy

**Automated and passing**:
- Startup-argument parsing/validation (`config.rs`), including the
  cross-platform absolute-path check (a real bug caught during
  implementation: hardcoded POSIX-style test literals like `/r` are not
  actually absolute paths on Windows — `Path::is_absolute()` requires a
  drive letter or UNC prefix there — fixed by deriving real absolute test
  paths from `std::env::current_dir()`, the same convention `guardian-cli`'s
  own tests already use).
- `resolve_store`'s OS-vs-vault selection and fail-closed-on-unopenable-vault
  behavior (`secret_store.rs`).
- The relocated `JobRegistry`'s existing register/cancel/drop-unregisters
  tests, unchanged.
- `excluded_tools_stay_excluded` (Decision 6).
- **A real MCP protocol round trip** over an in-memory `tokio::io::duplex`
  transport: a genuine `rmcp` client connects to a real `GuardianMcpServer`
  instance, calls `tools/list` and confirms the expected tool names are
  present, then calls `list_ssh_profiles` for real and confirms a
  non-error, structured response. This is a real client/server exchange
  through the actual wire protocol, not a direct Rust function call —
  confirmed feasible (this pattern is exactly what `rmcp`'s own upstream
  test suite uses) before committing to it as a verification strategy.

**Not exercisable without a live external MCP client, named honestly rather
than silently skipped** (matching ADR 0009/0010's own established pattern
for live-round-trip gaps): a real stdio handshake through an actual
subprocess launch (Claude Code, Claude Desktop, or any other real MCP
client spawning the compiled `guardian-mcp` binary), and real MCP-level
cancellation-notification propagation. Extending the existing clean-room
drill (`crates/guardian-capture/tests/clean_room_drill.rs`, ADR 0011) with
an MCP-driven variant — running capture/restore/deploy through real
`guardian-mcp` tool calls instead of calling the composition roots
directly — is valuable future work, not this slice.

## Consequences

Headless and programmatic access to capture/restore/deploy now exists for
the first time, without CLI growing a capture command or the desktop app
losing its status as the sole first-class human interface. The confirmation-
phrase safety gates every other surface already enforces are preserved
unchanged. No new trust boundary wider than "a local process the operator's
own session spawned" is created.

## Non-goals

Streamable HTTP or any network-reachable transport; any new authentication/
authorization model; tools that create new trust/config state (enrollment,
credential import, agent-key registration, repository registration, vault
init, signing enrollment, capture-plan creation); any auto-supplied or
bypassable confirmation; cancellation support for restore; the full unified
application-service crate (Decision 6); MCP progress-notification streaming
(tool calls stay synchronous, matching today's precedent); a standalone
`verify_backup` tool; any telemetry beyond the existing local audit trail
(`CODEX.md`: "No telemetry leaves the machine unless an explicit future
opt-in ADR allows it"); packaging/signing for the new binary.
