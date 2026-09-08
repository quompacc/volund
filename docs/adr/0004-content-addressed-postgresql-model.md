# ADR 0004: Content-addressed PostgreSQL persistence

## Status

Accepted on 2026-08-28.

## Context

VÖLUND indexes authoritative CAD files without renaming or moving them. The
same bytes may appear at several paths, paths may move, and preview generation
must not be repeated for duplicates. OBJ and glTF documents can also reference
sidecar material, buffer, and texture files.

Treating a path as the identity of a CAD model would couple durable metadata to
one filesystem layout. Treating every path as an independent model would waste
storage and conversion time.

## Decision

PostgreSQL separates physical observations from immutable content:

- `library_roots` identifies configured read-only source trees;
- `source_files` identifies a relative path observed below one root;
- `content_objects` identifies file bytes by normalized SHA-256;
- several source files may reference one content object;
- `file_dependencies` records sidecar references and their resolved source file;
- `scan_runs` records discovery history and last-seen state;
- `conversion_runs` belongs to content, not to a source path;
- `derived_artifacts` records rebuildable outputs of one conversion.

Internal relations use identity `bigint` keys. Entities likely to cross a future
API boundary also receive random UUID public identifiers. SHA-256 remains the
stable content identity and is protected by a unique database constraint.

Formats and states use text columns with `CHECK` constraints instead of native
PostgreSQL enums. This keeps additions expressible as ordinary forward
migrations. All timestamps use `timestamptz`; all source paths remain normalized
relative forward-slash paths.

SQLx owns a monotonically numbered migration history. A committed migration is
immutable and production startup never performs an implicit schema mutation.
Migration is an explicit administrative command.

## Consequences

Moving or renaming a file changes a path record without changing content-level
conversion identity. Duplicate files share conversion results. A scanner must
hash changed observations before associating them with content and must resolve
sidecar paths without escaping a configured root.

Logical user-facing projects, revisions, tags, and permissions are deliberately
not invented in the initial schema. They will be added when their behavior is
defined rather than conflated with filesystem discovery.
