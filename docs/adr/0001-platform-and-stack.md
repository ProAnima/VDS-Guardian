# ADR 0001: Rust core, Tauri desktop, React UI

- Status: accepted
- Date: 2026-07-13

## Context

The product must run on Windows and Linux, support a native desktop UI, operate
headlessly on backup nodes, stream large backup payloads, control SSH/processes,
handle hostile input safely, and share behavior across all surfaces.

## Decision

Use a Rust workspace for the domain, use cases, adapters, and headless CLI. Use
Tauri 2 as the desktop shell with a strict React/TypeScript/Vite UI. The WebView
has a narrow typed bridge and no generic shell/filesystem capability.

Prefer an embedded Rust SSH implementation only if interoperability testing is
strong enough; otherwise use the system OpenSSH client through direct argv and
strict configuration. This choice remains behind a port and will receive a
benchmark/security spike before Milestone 2.

Use independent tar+Zstandard payloads and manifests initially. Do not make
restic/Borg repositories the canonical format because their shared chunk stores
conflict with the requirement that each backup remain independently movable and
recoverable. They may later be optional replication/export adapters.

## Consequences

- GUI and service share backup and restore policy.
- Linux headless use does not require WebKit/Tauri packages.
- Rust compilation and platform packaging add CI cost.
- OS keyring, scheduling, and installer code require narrow platform adapters.
- The project must maintain hostile-input tests around archives and remote data.

## Rejected alternatives

- Electron/Node-only: faster UI iteration but larger runtime and weaker fit for
  a small privileged, headless-capable systems tool.
- Python/PySide: good prototyping speed but packaging, static guarantees, and
  long-running process/stream control are less aligned with the threat model.
- Two separate GUI and daemon implementations: unacceptable policy drift.
- Shared-chunk deduplicating repository as the only format: violates physical
  independence and increases blast radius of corruption/deletion.

