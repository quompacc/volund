# ADR 0001: The source filesystem is authoritative

- Status: Accepted
- Date: 2026-08-28

## Context

The archive will index thousands of existing CAD files. Requiring an irreversible
import into application-private storage would create migration risk and make the
application itself a single point of failure.

## Decision

VÖLUND indexes configured source roots in place. A source root starts read-only.
The application records normalized relative paths and content hashes, but never
uses a database identifier as the only way to locate an original.

Generated artifacts live under a separate application-data root and are addressed
by content hash. User metadata lives in PostgreSQL and must be completely exportable
to a documented format.

## Consequences

- Deleting VÖLUND does not make the CAD library unintelligible.
- A database can be rebuilt by rescanning originals.
- Renames are detected by matching content hashes.
- User metadata needs explicit export and restore tests.
- Write-enabled library management, if ever added, is a separate opt-in capability.

