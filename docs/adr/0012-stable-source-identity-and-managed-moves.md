# ADR 0012: Stable source identity and managed moves

- Status: Accepted
- Date: 2026-08-28

## Context

Model, collection, preview, and future PLM relationships must survive changes to
the physical folder layout. At the same time, imported files can occasionally be
classified into the wrong directory and need an explicit correction mechanism.
Using a relative path as identity would make these corrections destructive to
metadata relationships.

## Decision

`source_files.public_id` is the stable identity of an indexed physical file.
Relationships reference that UUID or the associated content object, never the
mutable `relative_path`.

Scanners and converters remain strictly read-only. A separate managed-move API
may relocate one available file only within its existing library root. The
destination directory must already exist. The move preserves the filename and
source-file row, refuses destination overwrites, serializes against scans and
other moves with the library advisory lock, updates the relative path in one
database transaction, and records the old and new paths in an audit table.

The filesystem operation uses a hard-link-first sequence. This guarantees that
an existing target is never replaced and enables rollback if database
persistence fails. It also restricts managed moves to one filesystem, which is
consistent with a single registered library root.

The API service receives write access only to the registered library mount.
Derived artifacts and deployed web assets remain read-only to that process.

## Consequences

- Physical reorganization does not invalidate previews or higher-level links.
- A failed database update restores the previous physical path.
- An unexpected host failure between link creation and source removal can leave
  two identical links, preferring recoverable duplication over data loss.
- Cross-library moves, automatic folder creation, bulk moves, dependency-aware
  project moves, and automatic import classification remain separate features.
- Existing folder views continue to be projections of current relative paths.
