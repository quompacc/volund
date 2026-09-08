# ADR 0016: Atomic import publication

- Status: Accepted
- Date: 2026-08-28

## Context

A reviewed import combines PostgreSQL state with hundreds of filesystem changes.
Targets may change after review, uploads may be damaged, and an existing CAD source
may need relocation without losing its stable UUID or previews.

## Decision

Confirmation locks the draft and target library with the same advisory-lock
namespace used by scans and managed moves. It rehashes every staged file and every
matched source, checks byte sizes, rejects symlinked or occupied targets, and then
publishes through hard-link-first moves without overwrites. Incoming storage and
the managed library therefore reside on the same native data filesystem.

Database registration occurs in one transaction after filesystem publication.
It creates or updates the logical model, records content and source rows, preserves
relocated source IDs, writes move audits, roles, tags and author metadata, and marks
the draft committed. Any pre-commit storage or database failure reverses published
files in reverse order. Staging is removed only after database commit succeeds.

## Consequences

- A stale plan cannot silently overwrite library content.
- Model links remain independent of physical path changes.
- The import endpoint is intentionally single-shot and can be retried after a
  detected conflict because its draft and staged files remain intact.
- Incoming and library directories must share a filesystem that supports hard
  links; this is enforced by the native LXC data-mount layout.
