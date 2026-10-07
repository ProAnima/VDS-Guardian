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
- Prepare an empty local repository directory and new restore destinations:
  one new absolute path on the disposable VDS and, when restoring to another
  server is in release scope, one new absolute path on a second disposable
  Linux VDS. None may contain production data. (The desktop app restores to a
  server path or in place; restoring into a local directory is a CLI/MCP
  operation and is not part of this test.)
- Obtain the host key's `SHA256:` fingerprint independently, for example
  `ssh-keygen -lf` on the host public key read through the provider console.
- Keep the recovery-bundle passphrase, SSH private key, and any SSH login
  password outside screenshots, screen recordings, logs, and the evidence
  record.

## Procedure

1. Run `scripts/verify-release-artifact.ps1` against the downloaded installer,
   `SHA256SUMS`, and expected publisher, then install it. Launch the installed
   application normally; do not run it elevated.
2. On **Overview**, confirm the readiness tiles report every missing
   prerequisite explicitly. Open the **Backups** view and its settings drawer
   (**Backup setup and recovery**). Under protection, **Prepare protection**
   with its acknowledgement; under storage, **Create storage** for the empty
   repository directory and confirm it shows `recovery ready` (use **Prepare
   recovery** if it does not). Under **Recovery bundle**, export with two
   matching passphrase entries, then store that bundle outside the repository
   directory.
3. Open **Servers** → **Add server**. Enter the name, address, port, and SSH
   user, then **Fetch the server's host key**. Compare the displayed
   `SHA256:` fingerprint with the independently obtained one; stop if they
   differ. Choose **SSH key** mode and the key file, tick the fingerprint
   acknowledgement, and **Add and verify server**. Confirm the success message
   reports SSH and `tar.zstd` capture verified and the server row shows the
   SSH-key login method. If password login is in release scope, repeat with
   **Login password** mode for a second disposable account and confirm its row
   shows the password login method.
4. In **Backups**, pick that server and storage in the toolbar, expand
   **Files** in the explorer tree, and select only the harmless absolute
   source path. Check the selection basket, then **Review backup** and confirm
   the resolved paths before **Create backup**. Wait for "Backup created and
   verified" and record the backup ID it reports; failed or cancelled work must
   not appear as a restore candidate.
5. Open **Restore**, select the repository and that verified backup, and check
   its contents. Choose **Separate path**, the disposable VDS as target server,
   and a new absolute remote destination path, then **Preview restore**. Check
   the visible server and destination facts before typing the exact
   confirmation phrase, then **Restore now**. Compare the restored files with
   the disposable source data over the independently pinned SSH connection.
6. If restoring to another server is in release scope, add the second
   disposable VDS as in step 3, then repeat the preview-and-confirm flow with
   it as target server
   and a new path, and verify those files the same way.
7. Restart the installed application. Confirm the repository, SSH profile(s),
   and sealed backup remain visible, while no passphrase, password,
   private-key contents, or recovery key is displayed.
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
