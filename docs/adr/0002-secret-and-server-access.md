# ADR 0002: Referenced credentials and least-privilege SSH

- Status: accepted
- Date: 2026-07-13

## Decision

Application configuration stores credential references, never private key bytes
or passphrases. Secrets use OS credential storage; operator-selected key files
remain outside the repository and are referenced by local configuration.

Each VDS should use a dedicated backup account with a pinned host key and a
reviewed least-privilege sudo policy. Root login and broad shell access are not
the default. Scheduled jobs cannot accept a new server identity.

## Rationale

Embedding a key in an open-source application, binary resources, or committed
settings would expose every server using that key and make rotation difficult.
Independent nodes should have independently rotatable credentials so compromise
of one node does not automatically compromise all nodes.

## Consequences

- First-run enrollment has an explicit secret-store and host-trust workflow.
- Portable settings exports cannot silently migrate credentials.
- Headless Linux installations need a documented Secret Service or encrypted
  vault fallback before unattended scheduling is production-ready.
