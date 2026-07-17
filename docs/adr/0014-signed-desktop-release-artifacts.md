# ADR 0014: Signed desktop release artifacts

- Status: Accepted
- Date: 2026-07-17

## Context

Release 0.1 distributes a desktop application that can access backup keys and
operate on remote servers. An unsigned installer or an installer whose checksum
was published separately from the release is not a trustworthy delivery path.
`CODEX.md` requires signed tags, checksums and an SBOM for packages; automatic
updates remain explicitly out of scope.

## Decision

Only a pushed `v*` tag may publish desktop release artifacts. The release
workflow runs the same canonical verification and Linux SSH/clean-room gates
as CI before packaging. It builds native Windows and Linux bundles, then:

- builds exactly one Windows MSI and one NSIS `.exe`, then signs both with an
  Authenticode credential supplied only through GitHub Actions secrets;
- builds exactly one Linux DEB and one AppImage, then creates and verifies
  detached OpenPGP signatures for both and for `SHA256SUMS`;
- generates `SHA256SUMS` only after platform signing is complete, and publishes
  the artifacts, signatures, and checksum file as one GitHub Release;
- generates an SPDX JSON SBOM with Anchore's Syft-backed `sbom-action` before
  checksums are calculated, so the SBOM is covered by the signed checksum;
- creates GitHub/Sigstore build-provenance attestations for the final release
  files after signing and checksum generation;
- fails before publishing if any required signing secret is unavailable.

The upload step uses an explicit file allowlist for those four package formats
and their Linux signatures; it never uploads the bundler's whole output tree.
Ordinary CI separately builds the same four formats as unsigned packaging
smoke tests, validates their exact count, and never uploads those candidates.

The workflow never enables Tauri updater endpoints or embeds updater signing
keys. Signing material is written only into the ephemeral runner workspace and
is removed after use. The repository stores only secret names and operational
instructions, never certificates, private keys, passphrases, or server URLs.

## Consequences

Repository maintainers must configure these GitHub Actions secrets before the
first tag release: `WINDOWS_SIGNING_CERTIFICATE_BASE64`,
`WINDOWS_SIGNING_CERTIFICATE_PASSWORD`,
`WINDOWS_TIMESTAMP_URL`, `LINUX_GPG_PRIVATE_KEY_BASE64`, and
`LINUX_GPG_PASSPHRASE`. A release tag with missing material fails closed and
creates no release. Actual signed-artifact and Windows-smoke evidence remains
required before Release 0.1 can be called production-ready.

The SBOM action is an additional build-time dependency only: it scans the
release-artifact directory, receives no product credentials, and produces one
SPDX JSON file published beside the installers. The provenance action uses a
short-lived GitHub OIDC/Sigstore credential; it does not reuse an installer or
OpenPGP signing secret. Consumers can verify it with `gh attestation verify`.
