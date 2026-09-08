# ADR 0011: Virtual folder catalog

- Status: Accepted
- Date: 2026-08-28

## Context

Large engineering libraries already have meaningful directory structures. A
useful catalog must preserve that organization, search recursively, and remain
responsive with thousands of files. Indexing must not rename, move, or otherwise
rewrite authoritative CAD originals to create its own hierarchy.

## Decision

Folders are virtual projections of the forward-slash `relative_path` values
already stored for indexed source files. The API exposes direct child folders
and direct child files for normal navigation. A non-empty search query switches
file results to recursive matching below the selected directory.

Directory scope, case-insensitive search, supported-format filtering,
deterministic sorting, and bounded pagination execute in PostgreSQL. Public
parameters are validated against fixed format, sort, and direction sets;
directory values must be normalized relative paths without traversal segments.

The browser encodes catalog state in ordinary query parameters. Back/forward
navigation and bookmarked views therefore work without a separate client-side
state service.

## Consequences

- Existing directory layouts appear immediately after indexing, with no schema
  migration and no duplicate folder records.
- Originals remain authoritative and read-only to indexing; explicit managed
  moves are defined separately in ADR 0012.
- Folder counts and recursive searches reflect only the current indexed view.
- Renaming or creating logical folders independently of the filesystem is not
  part of this model and requires a later metadata design.
