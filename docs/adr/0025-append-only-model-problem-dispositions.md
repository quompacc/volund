# ADR 0025: Append-only model problem dispositions

- Status: Accepted
- Date: 2026-08-30

## Context

Conversion diagnostics are immutable derived evidence, but an editor needs to
ignore, resolve, or reopen a problem without changing the original diagnostic.
The decision must retain actor and time, survive reload, reject stale model
state, and avoid an unbounded mutable shadow of preview data.

## Decision

Problem dispositions are append-only `model.problem.status` entries in the
existing model audit history. A bounded problem key identifies only a current
diagnostic belonging to a conversion run reached through that model's sources.
The newest entry for a key determines its displayed disposition; the diagnostic
itself remains unchanged.

Updates are editor-only, model-revision checked, CSRF and same-origin protected,
limited to 100 distinct current keys, and accept only `open`, `ignored`, or
`resolved`. Read responses bound the conversion runs, diagnostics, history, and
strings before serialization. Unknown, duplicate, foreign, and stale keys fail
closed.

## Consequences

- Every status change preserves actor, timestamp, revision, target, and prior
  history without another mutable lifecycle table.
- Reopening a problem appends evidence instead of erasing the resolution.
- Removing derived artifacts does not alter original bytes or catalog metadata;
  historical dispositions can remain as harmless audit evidence.
- This model does not provide comments, assignments, notification workflow, or
  cross-model issue tracking. Those remain outside Phase 5.
