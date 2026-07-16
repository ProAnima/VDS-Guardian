# 0011: Archive path validation hardening

Date: 2026-07-16

## Status

Accepted.

## Context

The clean-room drill (`crates/guardian-capture/tests/clean_room_drill.rs`,
added 2026-07-15) had never actually run to completion — first blocked by an
unrelated fixture bug (its Dockerfile named a nonexistent Alpine package,
`sqlite3` instead of `sqlite`). Once that was fixed, its first-ever real run
surfaced two independent, previously undiscovered defects in
`guardian-archive`, the shared library both capture-time archive inspection
(`TarZstdInspector::inspect`) and restore-time extraction
(`extract_tar_zstd`) depend on.

**Why no test ever caught either defect**: every existing unit and
integration test for `guardian-archive` built its fixture archives via this
project's own `TarZstdWriter`, whose input type (`ArchivePath`) cannot
represent the exact byte-level shapes real GNU tar produces. The clean-room
drill is the first test anywhere in this repository that feeds a
*genuinely* `tar`-produced archive (from the real remote capture command)
through this code. Both defects are assessed as having existed since this
validation logic was first written — not a regression introduced by any
change this session — simply never exercised until now.

## Defect 1 — directory entries were always rejected

Real tar writers (GNU tar, BSD tar) always suffix a directory member's own
name with `/` in the archive header — e.g. capturing `/srv/app` produces an
entry literally named `srv/app/`. `ArchivePath::parse`
(`crates/guardian-core/src/identifiers.rs`) validates a path by splitting on
`/` and rejecting any empty segment; a trailing slash produces exactly one,
so `inspect_entry`/`extract_entry`
(`crates/guardian-archive/src/lib.rs`) rejected every directory entry a real
capture ever produced, immediately failing archive inspection with
`ArchiveError::UnsafePath`.

**Blast radius**: any real capture of a directory containing at least one
subdirectory — the overwhelming majority of real-world capture roots — failed
this check the moment the drill exercised it against a live SSH round trip.

**Fix**: a new `parse_entry_path(path, is_directory)` helper strips exactly
one trailing slash before validating, but *only* for directory-type entries
(`header.entry_type().is_dir()`). A file entry whose name ends in `/` is not
real tar output and stays rejected — confirmed by a new adversarial test
(`("safe/trailing-slash/", EntryType::Regular)` in
`inspection_rejects_hostile_paths_and_link_entries`).

`TarZstdWriter::append` (`crates/guardian-archive/src/writer.rs`) was also
fixed to append the same trailing slash for `EntryType::Directory` — closing
the exact blind spot that hid this defect. `inspect_accepts_regular_files_and_directories`
now uses a directory entry literally named `"srv/app/"` (not `"srv/app"`) so
it exercises the real shape directly, independent of the writer round-trip.

## Defect 2 — extraction assumed every entry's parent already existed

`extract_entry` computed `output = destination.join(path.as_str())`, then
required `output.parent()` to already be a directory, failing closed with
`ArchiveError::UnsafePath` otherwise. Real tar only ever emits entries for
the capture root itself and its descendants — never separate entries for
path segments *above* a multi-segment root. Capturing `/srv/app` yields an
archive whose first (and only top-level) entry is `srv/app/` itself; there
is no separate `srv/` entry, because `/srv` was never itself an archive
member. `destination` starts empty (freshly created by `extract_tar_zstd`
immediately before extraction begins), so `destination/srv` never exists,
and the very first entry always failed.

**Blast radius**: any real capture whose root is more than one path segment
deep — again, nearly every real-world capture root (`/var/www/html`,
`/etc/nginx`, `/home/user/app`, ...) — failed restore-time extraction, even
after Defect 1's fix let capture-time inspection through.

**Fix**: a new `ensure_parent_directories(destination, parent)` walks from
`parent` up toward `destination`, collecting any missing ancestors, then
creates them top-down, applying the same `restrict_directory` hardening
(`0o700` on Unix) already applied to every other directory this extractor
creates. This is safe because `parent` is always `destination` joined with
a prefix of an already-`ArchivePath`-validated path (no `..`, no absolute
segments) — it can never resolve outside `destination`. It is also safe
against a symlink-planted-as-intermediate-component attack, because this
extractor already rejects every non-file, non-directory entry type
(`ArchiveError::UnsupportedEntryType`) outright — a symlink can never enter
the tree in the first place, so nothing this code walks through can ever be
one.

A new regression test, `extraction_creates_missing_ancestors_for_a_multi_segment_root`,
builds an archive containing *only* a `srv/app` directory entry (no separate
`srv` entry) and a file beneath it — deliberately not using the existing
`archive()` helper, which explicitly enumerates every level and would hide
this exact gap — and confirms extraction still succeeds.

## A related, but separate, finding: the drill fixture's own permissions

After both defects above were fixed, `deploy_drill` still failed, with the
remote staging `mkdir` itself failing (`SshError::CaptureFailed`). Direct
inspection of the fixture container found `/srv` was `root:root`
(`drwxr-xr-x`) while only `/srv/app` was chowned to the `backup` account —
so the `backup` user had no write permission on `/srv` itself, and the
deploy staging directory (`<parent>/.guardian-deploy-staging.<run_id>`, a
sibling of the target under `/srv`) could never be created.

This is **not a new precondition** — it has been true since the very first
deploy P0 fix ("the mktemp call is placed as a sibling inside the same
parent directory as the target", see `docs/adr/0007-remote-deploy-to-a-new-vds.md`'s
"three P0 correctness/security bugs fixed" amendment): both
the old single-push mechanism and the new staged protocol require the SSH
account to have write access to a deploy target's *parent* directory. The
fixture had simply never modeled this, because `deploy_drill` never reached
this step before. Fixed in `tests/drill-fixture/Dockerfile` by chowning
`/srv` itself (not just `/srv/app`) to `backup` — matching a realistic
production setup where the backup account has been granted write access to
wherever it deploys, per ADR 0002's existing least-privilege framing.

## A related test-design correction: local database verification

Once both defects were fixed, `restore_drill` still failed — but on a plain
assertion, not an error: the restored `database.sqlite` did not byte-match
the seeded file fetched via `docker cp`. Direct byte comparison (`cmp -l`)
found exactly two differing 4-byte fields, at the exact offsets of
SQLite's own header "file change counter" and "version-valid-for number"
fields. A `.backup` (what the remote capture command runs) is a *logical*
copy through the database engine, not a raw byte copy — it is not
guaranteed, and was never actually guaranteed, to reproduce these
bookkeeping fields verbatim, only the data they describe. This makes the
original test design's assumption ("byte-exact equality is as strong a
proof as `PRAGMA integrity_check`") incorrect for this field specifically.

Fixed by verifying the restored database with real SQL instead — added
`rusqlite` (bundled) as a dev-dependency, and `restore_drill` now runs
`PRAGMA integrity_check` (expects `ok`) and selects the seeded row,
mirroring `deploy_drill`'s own remote verification strategy rather than
assuming byte-for-byte equality. The filesystem payload's own byte-exact
comparison is untouched and remains correct — a plain file round-trips
through tar+zstd losslessly, with no intervening database engine to
introduce this class of difference.

## Consequences

Both `restore_drill` and `deploy_drill` now pass end-to-end for the first
time since either was written — real capture, real seal, real local
restore and real remote deploy, each independently verified. This closes
Release 0.1 section 1's regression-test bullet in full and gives its Gate
line genuine, not merely aspirational, evidence. It does not, on its own,
satisfy Release 0.1 section 5's full exit gate: only one fixture shape
(a two-level filesystem root plus one embedded database) has been proven
this way, rollback is still unproven (unbuilt), and the drill has not yet
been observed passing on a real Linux CI run — the next CI run on that leg
is the actual gate for that specific claim, matching how this project has
treated the drill's own CI wiring since it was first added.

## Non-goals

No change to `ArchiveLimits`, to the manifest/payload schema, or to any
already-reviewed capture/restore/deploy composition logic. No change to
what entry types are accepted (symlinks and hard links remain rejected
outright, unchanged). No attempt to make `TarZstdWriter`'s output
byte-identical to real GNU tar beyond the one property (directory trailing
slashes) this defect required — it remains this project's own internal test
fixture format, not a general-purpose tar writer.
