# Contributing

Thank you for helping build VDS Guardian.

1. Read `AGENTS.md`, `CODEX.md`, and the relevant architecture/security docs.
2. Open an issue for behavior or format changes before implementing them.
3. Keep each change focused and include tests and documentation updates.
4. Run `npm run doctor` and `npm run verify` before submitting a pull request.
5. Explain threat-model impact for any change touching SSH, archives, secrets,
   storage lifecycle, retention, updates, or restore.

Commits should use imperative subjects. Pull requests should state what changed,
why, how it was verified, and what remains unverified. Never include real
credentials, server addresses, logs containing secrets, or backup payloads.

