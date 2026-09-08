# ADR 0005: Read-only incremental library scanning

- Status: Accepted
- Date: 2026-08-28

## Context

VÖLUND must index large, user-owned CAD libraries without turning the database
into the authority for original bytes. Rehashing every file on each scan would
make routine scans unnecessarily expensive, especially for multi-hundred
megabyte assemblies. Files may also disappear temporarily because a mount is
offline or a user reorganizes the authoritative filesystem.

## Decision

The scanner traverses registered roots without modifying their contents and
indexes only supported CAD extensions. It never follows filesystem symlinks.
Stored paths are normalized relative paths, while root mount paths remain
deployment configuration.

An incremental scan reuses an existing content identity when byte size and
filesystem modification time are unchanged. New and changed files are hashed
with streaming SHA-256. `scan --full` bypasses that optimization and hashes all
supported files. Metadata is checked again after hashing; a concurrent change
fails the scan.

Filesystem discovery and hashing complete before one database transaction
updates content objects, source paths, missing markers, counters, and scan
status. A pre-commit failure records a failed scan run but leaves the previously
successful library view unchanged. Identical hashes share one content object.
Paths absent from a successful scan are marked missing, never deleted.
An advisory database lock permits only one active scan per library root.

## Consequences

- Routine scans avoid rereading unchanged large assemblies.
- A full scan provides explicit content verification when desired.
- Symlink exclusion prevents traversal outside a registered authority boundary.
- Size and modification time are an optimization hint, not a cryptographic
  guarantee; scheduled or manual full scans remain useful.
- Historical source paths and scan outcomes remain auditable after files vanish.
