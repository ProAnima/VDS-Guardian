<p align="center">
  <img src="docs/assets/readme-header.svg" alt="VDS Guardian — isolated backups and verified recovery" width="100%" />
</p>

<p align="center">
  <a href="https://github.com/ProAnima/VDS-Guardian/actions/workflows/ci.yml"><img src="https://github.com/ProAnima/VDS-Guardian/actions/workflows/ci.yml/badge.svg" alt="CI status" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-70ddb7.svg" alt="Apache-2.0 license" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-102a25.svg" alt="Windows and Linux" />
  <img src="https://img.shields.io/badge/status-0.1%20hardening-e2b35d.svg" alt="Release 0.1 hardening" />
</p>

VDS Guardian is an open-source desktop and headless disaster-recovery manager
for remote Linux servers. It is built for operators who need isolated,
independently stored recovery points and a predictable path from a failed or
compromised VDS to a clean, working deployment.

The first release focuses on one manual path: add a Linux server, browse its
filesystem and Docker-backed persistent data, capture an explicit selection,
store a verified backup on a local or removable disk, and restore it to a new
destination without a mandatory cloud service.

> **Project status:** Release 0.1 hardening. Pinned SSH capture, encrypted sealed
> backups with a portable per-repository recovery key, local restore,
> new-host deploy, optional SQLite snapshots, a guided desktop setup/recovery
> flow, and
> an automated clean-room drill (including compiled-CLI recovery import on a
> clean vault and registry, now passing end-to-end locally and on Linux CI)
> are implemented foundations. The remaining Release 0.1 blockers include
> signed Windows/Linux installer artifacts with published checksums and the
> release-candidate Windows desktop smoke evidence. Do not use the application
> as a disaster-recovery system until the Release 0.1 exit gate passes.

## Design goals

- One Rust backup engine shared by the desktop GUI, a headless CLI
  (enrollment/restore/deploy), and an MCP server for programmatic and
  AI-agent access.
- First-class Windows and Linux support; no WSL requirement on Windows.
- A separate sealed directory for every completed backup.
- A staging-to-sealed lifecycle so failed or suspicious runs never become
  restorable backups.
- SHA-256 manifests, signed metadata, verification, quarantine, and restore
  drills.
- A concise Servers view plus a bounded filesystem/Docker explorer that resolves
  visual selections to explicit paths, with an optional SQLite snapshot.
- Portable recovery material so an independent backup disk remains restorable
  after loss of the original operator machine.
- SSH private keys live in the OS credential store or an operator-selected
  file outside the repository. Secrets are never embedded in source code or
  committed configuration.
- Destructive actions are dry-run-first, auditable, and require explicit
  confirmation.

## Chosen stack

- **Core and CLI:** Rust 2024 edition.
- **Desktop shell:** Tauri 2.
- **UI:** React 19, strict TypeScript, Vite.
- **Remote transport:** OpenSSH-compatible SSH/SFTP behind a narrow Rust port.
- **Backup payload:** deterministic `tar` + Zstandard streams, one payload per
  backup, with a versioned manifest. Database-aware adapters use native dump
  tools instead of copying live database files.
- **Secrets:** Windows Credential Manager / Secret Service-compatible keyring,
  with an encrypted local vault fallback for headless nodes without one.

The architecture decision and rejected alternatives are documented in
[`docs/adr/0001-platform-and-stack.md`](docs/adr/0001-platform-and-stack.md).

## Repository layout

```text
apps/desktop/          Tauri desktop shell and React UI
crates/guardian-core/  Domain model and use cases; no UI or Tauri dependency
crates/guardian-local-repository/  Cross-platform staging and seal adapter
crates/guardian-signing/  Ed25519 backup-node identity lifecycle
crates/guardian-os-keyring/  Windows/Linux secure credential-store adapter
crates/guardian-vault/  Encrypted local file vault fallback for headless nodes
crates/guardian-cli/   Headless enrollment/restore/deploy/recovery entrypoint
crates/guardian-mcp/   MCP server for headless/AI-agent capture/restore/deploy
docs/                  Architecture, security, backup format, and roadmap
scripts/               Canonical doctor and verification entrypoints
```

## Development

Prerequisites: Node.js 22+, npm 10+, Rust 1.96, and the platform prerequisites
listed by Tauri 2. On Linux, WebKitGTK development packages are required for the
desktop shell; the headless CLI does not need them.

```powershell
npm.cmd install
npm.cmd run verify
npm.cmd run dev
```

Linux:

```bash
npm install
npm run verify
npm run dev
```

The canonical gates are described in `AGENTS.md`. Do not replace them with an
informal list of partial checks.

Signing identity inspection is read-only and never enrolls implicitly. The
headless CLI requires an explicit absolute node-configuration path:

```powershell
guardian-cli signing status --config-dir D:\VDSGuardian\node --json
guardian-cli signing enroll --config-dir D:\VDSGuardian\node --json
```

On headless Linux without a usable Secret Service, opt into the encrypted
local vault fallback instead (ADR 0006). It must be initialized once, then
selected explicitly on every command that needs a credential store:

```powershell
guardian-cli vault init --vault-dir D:\VDSGuardian\vault --json
guardian-cli vault status --vault-dir D:\VDSGuardian\vault --json
guardian-cli signing enroll --config-dir D:\VDSGuardian\node --vault-dir D:\VDSGuardian\vault --json
```

Pinned VDS profiles are also enrolled through explicit JSON commands. The input
document contains public endpoint data, a credential reference, and an already
verified host-key pin; it never contains private key material. This is profile
setup only: it does not discover a host key or start a backup.

```powershell
guardian-cli profile enroll --profiles-dir D:\VDSGuardian\profiles --input D:\VDSGuardian\profile.json --json
guardian-cli profile list --profiles-dir D:\VDSGuardian\profiles --json
```

Importing a dedicated SSH key is a separate, explicit operation. The key is
stored only in the OS credential store under the profile's credential ID; an
existing credential is never overwritten. The current foundation accepts only
unencrypted OpenSSH private keys, and does not yet support rotation.

```powershell
guardian-cli credential import-ssh-key --credential-id credential-001 --input D:\VDSGuardian\backup.key --json
```

A passphrase-protected key is supported instead through an already-running
OS SSH agent (ADR 0009): register only its public key, and keep the
matching private key loaded in the agent at connection time. VDS Guardian
never sees the passphrase. Limited to `ssh-ed25519`/`ecdsa-sha2-
nistp256/384/521` identities for now; there is no desktop UI for this path
yet.

```powershell
guardian-cli credential register-agent-key --credential-id credential-002 --public-key-file D:\VDSGuardian\backup.pub --json
```

Every repository needs a configured recovery key (ADR 0013) before its first
encrypted capture; live capture fails closed otherwise. Export it into a
passphrase-protected offline bundle and keep that bundle independent of the
repository disk — an intact backup is only as recoverable as its
independently stored recovery material:

The guided desktop setup can initialize repository recovery, export an offline
bundle, and import it on a clean machine. Import authenticates the bundle
before registering an unknown repository; the passphrase stays in memory for
the operation. The commands below are the equivalent headless workflow.

```powershell
guardian-cli recovery init --repositories-dir D:\VDSGuardian\repositories --repository-id repository-001 --signing-config-dir D:\VDSGuardian\node --json
guardian-cli recovery export --repositories-dir D:\VDSGuardian\repositories --repository-id repository-001 --passphrase-file D:\VDSGuardian\passphrase.txt --output D:\VDSGuardian\recovery-bundle.json --confirmation "EXPORT RECOVERY BUNDLE FOR repository-001" --json
```

On a clean machine that has the repository directory and the bundle, but no
state from the original machine's OS credential store:

```powershell
guardian-cli recovery import --repositories-dir D:\VDSGuardian\repositories --repository-id repository-001 --repository-path E:\VDSGuardianBackup --input D:\VDSGuardian\recovery-bundle.json --passphrase-file D:\VDSGuardian\passphrase.txt --confirmation "IMPORT RECOVERY BUNDLE FOR repository-001" --json
```

The pinned SSH profile is the only VDS transport boundary. Live filesystem
capture, optional SQLite snapshot, local restore, and deploy to a new remote
destination are implemented foundations, but they are not production-ready.
PostgreSQL/MySQL probes and Docker discovery exist as later-work foundations;
their dump/restore and consistency workflows are outside Release 0.1.

## Security boundary

VDS Guardian assumes the remote server may already be compromised. A backup
worker therefore receives only the access required by its reviewed backup plan,
never executes data from the backup, writes only to a fresh staging directory,
and publishes it by an atomic rename after verification. Completed backup
directories are not reused or modified by normal application flows.

Storage isolation limits propagation and operator mistakes; it is not a malware
scanner and cannot make a backup trustworthy by itself. See
[`docs/SECURITY_MODEL.md`](docs/SECURITY_MODEL.md) for the full threat model and
[`docs/SIGNING_IDENTITY.md`](docs/SIGNING_IDENTITY.md) for the node-key contract.
Retention safety and interrupted-cleanup behavior are specified in
[`docs/RETENTION.md`](docs/RETENTION.md).

## Roadmap

The milestone plan, acceptance gates, and definition of done are in
[`docs/DEVELOPMENT_PLAN.md`](docs/DEVELOPMENT_PLAN.md).

## Contributing

Read `AGENTS.md`, `CODEX.md`, and `CONTRIBUTING.md` before changing code. Security
issues should follow `SECURITY.md` and must not be filed as a public issue —
report them through a GitHub security advisory instead.

## License

Licensed under the [Apache License 2.0](LICENSE).
