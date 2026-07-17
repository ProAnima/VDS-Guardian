# Windows desktop smoke test

Status: Release 0.1 manual release-evidence procedure. Running this procedure
does not make a release production-ready by itself; record its result together
with the same-commit CI and clean-room drill evidence.

## Preconditions

- Use a clean Windows 11 operator account or VM, not the build workstation.
- Use the candidate's **signed** installer and the published SHA-256 checksum.
  Stop if the installer is unsigned, its publisher is unexpected, or the hash
  differs. Do not substitute a development `tauri dev` build for this test.
- Prepare a disposable Linux VDS with a dedicated SSH backup account, a pinned
  host key obtained by an independent channel, and harmless source data under
  an absolute path such as `/srv/guardian-smoke`.
- Prepare an empty local repository directory and two new destinations: one
  local Windows path and, when deploy is included, one new absolute path on a
  second disposable Linux VDS. None may contain production data.
- Keep the recovery-bundle passphrase and SSH private key outside screenshots,
  screen recordings, logs, and the evidence record.

## Procedure

1. Verify the installer SHA-256 with `Get-FileHash -Algorithm SHA256`, verify
   the Authenticode publisher in Windows Explorer, then install it. Launch the
   installed application normally; do not run it elevated.
2. In **Setup status**, confirm every prerequisite is reported explicitly. Create
   the local signing identity, add the repository, and ensure its recovery key
   is ready. Export the recovery bundle with two matching passphrase entries,
   then store that bundle outside the repository directory.
3. Add the disposable SSH server with the independently verified host key and
   key file. Confirm the SSH and required capture capability checks succeed.
4. Save a capture plan for only the harmless absolute source path. Run it and
   wait for a sealed, verified backup. Record the backup ID; failed or
   cancelled work must not appear as a restore candidate.
5. Open **Restore**, select that verified backup, and preview a new local
   destination. Check the backup ID, destination, payload description, and
   rollback posture before typing the exact confirmation phrase. Complete the
   restore and compare the restored files with the disposable source data.
6. If deploy is in release scope, repeat the preview-and-confirm flow against
   the second disposable VDS and a new target path. Verify the deployed files
   over the independently pinned SSH connection.
7. Restart the installed application. Confirm the repository, SSH profile, and
   sealed backup remain visible, while no passphrase, private-key contents, or
   recovery key is displayed.
8. Uninstall the application. Do not delete the repository or recovery bundle;
   this is an installer smoke test, not a destructive recovery test.

## Evidence record

Record the following without secrets or server addresses:

| Field | Required value |
| --- | --- |
| Release tag and commit | Exact candidate identifier |
| Windows edition/build | Output of `winver` |
| Installer filename and SHA-256 | Value verified before install |
| Authenticode publisher | Expected release publisher |
| Backup ID | Sealed smoke-test backup ID |
| Restore/deploy result | Success or the safe failure/remediation shown by the app |
| Operator and timestamp | Person who performed the test and UTC time |

Any failed host-key, checksum, signature, restore preview, or cleanup check is
a release blocker. Preserve only redacted diagnostics and report the failure;
do not work around a security check to finish the smoke test.
