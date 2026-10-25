# ADR 0029: Portable model packages and versioned metadata export

- Status: Accepted
- Date: 2026-10-25

## Context

The product contract promises that originals and user-created metadata have a
documented, machine-readable exit path that does not depend on VÖLUND:

1. a model can be exported with originals, metadata, relationships, and a
   documented manifest;
2. an instance metadata export is machine-readable and versioned;
3. derived artifacts may be omitted and rebuilt.

The daily `pg_dump` (docs/BACKUP.md) protects the database, but it needs
PostgreSQL and the VÖLUND schema to be read. It is not an exit format.

## Decision

There are two complementary exports. Both use public UUIDs only, camelCase
keys and `*UnixMs` timestamps, and both have a JSON Schema in `contracts/`.

**Model package** (`GET /api/v1/models/{modelId}/export`, format
`volund-model-export-v1`). It is a zip containing:

- every available original below `originals/<rootKey>/<relativePath>`;
- `SHA256SUMS`, which can be checked with `sha256sum -c`;
- `manifest.json` with the model as the API returns it, its components, and
  every linked file with role, per-file metadata, SHA-256, and inclusion state.

Every authenticated role may export, because the package contains only what
`GET /models/{id}` and `/files` already show and allow to download.

Originals are resolved exactly like source streaming: the canonical path must
stay below the canonical library root, and the lifecycle state must be
`available`. Each original is verified against its catalog SHA-256 and size
while it is copied. A mismatch aborts the export with
`model_export_source_changed` instead of shipping a silently different file. A
missing or quarantined file is listed with `included: false` and an
`exclusionReason`, so the package stays honest without blocking the remaining
files.

The archive is written to `<support_root>/.model-export-*.partial`, because that
is the daemon's only writable non-library directory in the systemd sandbox. The
file is opened and immediately unlinked before streaming, so nothing remains even
if the client disconnects. At most two exports run concurrently; a third gets 409
`model_export_busy`. Success and failure are audited as `model.export`.

**Instance metadata** (`POST /api/v1/exports/metadata`, owner/administrator,
CSRF, format `volund-metadata-v1`). This is one JSON document built from a single
`REPEATABLE READ, READ ONLY` transaction, so it is a consistent snapshot. It
contains:

- library roots (key and display name), source files with hashes, file
  dependencies, and moves;
- authors (with merge aliases), tags, and collections with ordered members;
- models with license, thumbnail selection, tags, components, and per-file
  metadata;
- users without secrets;
- catalog history restricted to the same target types and metadata allowlist as
  the history API.

It omits credentials, sessions, invitations, slicer tokens, host filesystem
paths, derived artifacts, conversion runs, import drafts, jobs, logs, and
settings, and names them in `omitted`. The export is audited as
`metadata.export` with section counts.

## Consequences

- An operator can leave VÖLUND with a metadata JSON plus the library volume, or
  hand over single models as self-describing zip files. Both formats are
  verifiable with standard tools.
- Whole-instance packages that include originals are not produced. The library
  volume already holds the originals in readable paths, which `sourceFiles`
  references by `rootKey` and `relativePath`. A durable export job with a
  dedicated export directory, retention, and a new sandbox write path is
  deferred until large-library exports need it.
- Re-importing a `volund-metadata-v1` document into a fresh instance is not
  implemented. Disaster recovery still restores the PostgreSQL dump.
- Model packages are staged on the support volume. Operators must size
  `/srv/volund/support` for the largest single model they intend to export.
