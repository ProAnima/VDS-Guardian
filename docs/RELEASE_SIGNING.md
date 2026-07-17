# Release signing setup

The tag-only release workflow in `.github/workflows/release.yml` cannot publish
until every required GitHub Actions secret is configured. It fails closed rather
than uploading unsigned bundles.

| Secret | Purpose |
| --- | --- |
| `WINDOWS_SIGNING_CERTIFICATE_BASE64` | Base64 of the Authenticode PFX |
| `WINDOWS_SIGNING_CERTIFICATE_PASSWORD` | Password for that PFX |
| `WINDOWS_TIMESTAMP_URL` | RFC 3161 timestamp service URL |
| `LINUX_GPG_PRIVATE_KEY_BASE64` | Base64 of the armored OpenPGP private key |
| `LINUX_GPG_PASSPHRASE` | Passphrase for the OpenPGP key |

Keep the Windows certificate and Linux OpenPGP key in separate access-controlled
stores. Grant release-workflow access only to maintainers authorized to publish
the project. After configuring the secrets, create a protected signed `v*` tag;
the workflow runs verification and the clean-room drill before creating the
GitHub Release.

Before signing credentials are available, Windows packaging can be preflighted
locally with `npm run tauri:build:windows --workspace @vds-guardian/desktop`. A successful
preflight must produce both MSI and NSIS bundles under `target/release/bundle`;
they are intentionally unsigned candidates and must never be published as a
release or used as signed-smoke-test evidence.

Linux CI uses the matching `tauri:build:linux` script and accepts exactly one
DEB and one AppImage. Release packaging uses these same platform scripts,
verifies every signature immediately, and uploads only the explicit package
allowlist rather than the entire bundler output directory.

The workflow publishes platform signatures, an SPDX JSON SBOM, and
`SHA256SUMS` plus its detached OpenPGP signature. Do not enable Tauri
auto-updates or add updater keys as part of this procedure. GitHub also records
a Sigstore-backed build-provenance attestation for the final release files;
consumers can inspect it with `gh attestation verify <asset> -R
ProAnima/VDS-Guardian`. The release-candidate Windows smoke execution remains
open, so passing the workflow alone is not a production-readiness claim.

Release assets are deliberately flattened before `SHA256SUMS` is generated, so
their filenames match what a consumer downloads from GitHub Releases. On
Windows, verify an installer and publisher before execution:

```powershell
.\scripts\verify-release-artifact.ps1 -InstallerPath .\VDS-Guardian-setup.exe -ChecksumsPath .\SHA256SUMS -ExpectedPublisher "ProAnima"
```
