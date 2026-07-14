# ADR 0003: Major-version parity for database dumps

- Status: accepted
- Date: 2026-07-14

## Decision

VDS Guardian permits PostgreSQL and MySQL dump capture only after read-only
preflight confirms that the server and the selected dump tool have the same
major version. A missing, malformed, or mismatched version fails the capture
plan rather than attempting a best-effort dump.

## Rationale

Client/server compatibility varies by engine, distribution, and release. A
strict major-version rule is predictable, inspectable, and safer than relying on
tool-specific fallback behavior before fixture restore drills exist.

## Consequences

- Operators must provide a matching dump tool in the reviewed adapter image or
  remote environment.
- Later compatibility exceptions require evidence, regression fixtures, and an
  ADR update.
- This decision checks compatibility only; it does not make a dump consistent,
  encrypted, or restorable.
