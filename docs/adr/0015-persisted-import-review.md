# ADR 0015: Persisted import review

- Status: Accepted
- Date: 2026-08-28

## Context

A completed upload is not sufficient authority to mutate the CAD library. The
system must explain deduplication, existing-model reuse, physical destinations,
and conflicts before the user confirms a potentially large import.

## Decision

VÖLUND persists a deterministic review plan after every staged item has a
verified SHA-256. The primary file's existing source, when present, selects the
library root and parent namespace. Otherwise the first configured root and a
kind-specific `Bauteile`, `Baugruppen`, or `Projekte` directory are used.

An existing model with the same slug is updated rather than duplicated. Each
item receives one action: create a new source, reuse an existing source,
relocate an identical same-root source while preserving its UUID, or block on a
different file occupying the target path. Redundant filename directories are
collapsed in target proposals.

The review writes only database planning fields. It does not move, create, or
delete library files. The later commit must lock the draft and revalidate every
source hash, target path, and staged file before applying the plan.

## Consequences

- Users can inspect exact physical and logical effects before confirmation.
- Identical large CAD files keep their existing source UUID and previews.
- Review remains safely repeatable as external state changes.
- A separate atomic commit and rollback mechanism is still required.
