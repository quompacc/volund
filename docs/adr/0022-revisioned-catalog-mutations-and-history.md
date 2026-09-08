# ADR 0022: Revisioned catalog mutations and bounded actor history

- Status: Accepted
- Date: 2026-08-30

## Context

Phase 3 lets several authenticated users maintain shared model metadata and
global vocabularies. Row locking alone serializes writes but does not tell a user
that a form was based on stale data. Existing security audit events have the
required immutable actor snapshot and safe metadata boundary, but catalog writes
did not use them.

## Decision

Every revisioned catalog object stores a positive monotonically increasing
revision. A write supplies the revision it observed, locks the target row, and
fails with `revision_conflict` before changing related rows when the value no
longer matches. Relationship and merge operations lock all involved rows in a
stable order.

Catalog mutations append an event to `security_audit_events` inside the same
database transaction as the business change. Events contain actor identity,
stable action and target identifiers, outcome, and a small allowlisted change
object. They never contain request headers, submitted confirmations, complete
before/after objects, source contents, or host paths.

Authenticated viewers may read model-scoped successful catalog history because
it contains catalog facts already visible to them. Denied critical transitions
remain security-administration evidence and are not exposed by model history.

## Consequences

- Parallel stale forms cannot silently overwrite newer catalog state.
- Domain changes and their actor evidence commit or roll back together.
- The existing append-only table avoids a second competing audit authority.
- Additive revisions are ignored by older binaries, preserving rollback to the
  previous release after migration.

