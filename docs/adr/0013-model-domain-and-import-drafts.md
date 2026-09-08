# ADR 0013: Model domain and metadata-only import drafts

- Status: Accepted
- Date: 2026-08-28

## Context

VÖLUND needs logical models, collections, authors, and tags that do not depend
on physical paths. It also needs to propose a clean project layout before large
files or archives are written to authoritative storage. Mixing analysis and
physical import would make classification mistakes expensive to correct.

## Decision

Models are persistent PostgreSQL entities. Their source relationships reference
stable `source_files` rows and record semantic roles such as primary master CAD,
printable mesh, document, image, or archive. Collections, authors, and tags are
independent many-to-many metadata where appropriate.

Import analysis is a separate metadata-only draft. The browser submits a bounded
manifest of normalized relative paths and byte sizes. The server rejects unsafe,
duplicate, negative, oversized, or excessive entries; removes a common package
root; classifies each file; proposes a deterministic model slug and category
directory; and marks the largest native CAD file as the primary candidate.

Drafts and their item proposals are persisted transactionally. Creating a draft
does not read, upload, copy, move, or otherwise mutate source bytes. Streamed ZIP
inspection, upload staging, user confirmation, collision handling, hashing, and
atomic library commit build on this contract in later milestones.

The review step persists the chosen model name and derived slug, model kind,
description, optional author, and normalized tags on the draft. These values are
independent of target paths and do not create a model until the later atomic
commit succeeds.

## Consequences

- Logical organization remains independent from physical storage paths.
- Classification proposals can be inspected and refined without touching CAD
  originals.
- Large browser selections remain bounded because only metadata is transferred.
- A ZIP selected as one file is currently classified as an archive; its internal
  manifest becomes available only after the streamed archive-ingestion stage.
- Draft retention and cleanup policy must be added before unattended production
  imports are enabled.
