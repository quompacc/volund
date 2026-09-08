# ADR 0007: Durable single-worker preview queue

- Status: Accepted
- Date: 2026-08-28

## Context

CAD conversion is resource-intensive and must survive API restarts without
duplicating work or losing its state. Original files may also change between a
library scan and conversion.

## Decision

Preview requests are stored in PostgreSQL and identified by public UUIDs. A
partial unique index permits only one queued or running request per content hash
and quality profile. A request reuses a ready result produced by the current
converter and contract version.

The native worker claims one queued row with `FOR UPDATE SKIP LOCKED`, then
recalculates the source SHA-256 before invoking the existing flock- and
timeout-guarded conversion runner. It publishes five artifacts only after all
are present and individually hashed. The database transition to `ready` and all
artifact rows commit atomically. Failures retain diagnostics. Running jobs older
than two hours are marked failed before another claim.

A systemd oneshot service and timer poll the queue. The service sees the CAD
library read-only and can write only derived and scratch storage.

## Consequences

- API and worker restarts do not lose queued work.
- Duplicate clicks or automation do not launch duplicate active conversions.
- A stale scanner observation cannot silently convert different bytes.
- Only one conversion is processed per service invocation; systemd and the
  existing global flock keep resource use predictable.
- Ready artifact metadata is queryable now; authenticated enqueue and HTTP
  artifact streaming remain separate contract additions.
