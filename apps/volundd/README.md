# VÖLUND job runner

## PostgreSQL persistence

Schema changes are explicit administrative operations:

```sh
sudo -u volund /usr/local/bin/volundd migrate
sudo -u volund /usr/local/bin/volundd database-doctor
```

With no additional configuration, `volundd` connects to the local Unix socket
as PostgreSQL role `volund` and database `volund` using peer authentication.
`VOLUND_DATABASE_URL` overrides this only for isolated tests or an intentional
alternate deployment; credentials must never be committed.

Migration 0001 creates content-addressed storage for library roots, scan runs,
physical source paths, shared content hashes, sidecar dependencies, conversion
runs, and derived artifacts. Migrations run only through `volundd migrate`, not
as a side effect of daemon startup.

Migration 0003 adds the audit trail for managed same-library source moves. The
source UUID and content relationships remain unchanged when its path changes.

Migration 0004 adds logical models, stable file roles, authors, collections,
tags, and metadata-only import drafts. Draft creation validates and classifies a
manifest but never writes source bytes.

Migration 0005 adds the user-reviewed model metadata to import drafts. Saving
these fields marks the plan as ready for a later upload but still writes no
original file bytes.

Migration 0006 assigns opaque upload IDs and persistent staging status, byte
counts, and SHA-256 values to every draft item. File bodies stream into the
dedicated `/srv/volund/incoming` data-mount directory.

Migration 0007 persists the final pre-commit review: target root and base path,
existing model reuse, per-file create/reuse/relocate/conflict actions, and stable
source matches. Review remains read-only with respect to library files.

Migration 0008 records the successful publication time. The commit endpoint
rehashes staged and matched files, refuses overwrites, publishes on the shared
data filesystem, registers every project file and rolls changes back on failure.

Linux integration tests require a disposable database explicitly supplied as
`VOLUND_TEST_DATABASE_URL`. Without it, database-mutating tests skip safely.
The database name must contain `test` or `audit`; the shared harness serializes
test binaries and clears application rows before every database test.

## Catalog HTTP API

`volundd serve` exposes the versioned catalog API on `127.0.0.1:8080` by
default. It verifies schema health at startup but never runs migrations.

```sh
volundd serve
curl http://127.0.0.1:8080/api/v1/health
curl http://127.0.0.1:8080/api/v1/roots
curl 'http://127.0.0.1:8080/api/v1/roots/cad/files?limit=50&offset=0'
curl 'http://127.0.0.1:8080/api/v1/roots/cad/scans?limit=20'
curl http://127.0.0.1:8080/api/v1/content/<sha256>
```

`POST /api/v1/files/{fileId}/move` accepts an
existing destination directory inside the same library, refuses overwrites,
rolls the filesystem back if persistence fails, and audits both paths. Scans,
previews, and all other catalog endpoints remain read-only.

`GET/POST /api/v1/models` exposes the first persistent logical model contract.
`POST /api/v1/imports/preview` persists a bounded metadata-only classification
and target-layout proposal; it is not a file-upload endpoint.
Import metadata must select a registered library by its stable `libraryRootId`.
Review and commit retain that exact target and never fall back to another root.
`POST /api/v1/imports/{draftId}/metadata` stores the reviewed model name, kind,
description, author, and tags before transfer begins.
`POST /api/v1/imports/{draftId}/items/{itemId}/content` streams one reviewed
item into staging and refuses a missing or mismatched `Content-Length`.
`GET /api/v1/imports/latest-uploaded` resumes the newest complete staging draft.
`POST /api/v1/imports/{draftId}/review` persists and returns its deterministic
deduplication and target-path plan without committing it.
`POST /api/v1/imports/{draftId}/commit` performs the locked, revalidated,
single-shot publication and logical model registration.

The machine-readable contract is
[`contracts/http-api-v1.openapi.yaml`](../../contracts/http-api-v1.openapi.yaml).
Set `VOLUND_LISTEN_ADDR` only for an intentional alternative listen address.

The production browser build is read from `/usr/share/volund/web` by default
and served with SPA fallback and restrictive browser security headers. Override
the path with an absolute `VOLUND_WEB_ROOT` only for development or packaging.

## Durable preview queue

Preview generation is deliberately separated from the API process:

```sh
volundd enqueue-preview --file <source-public-id> --profile web
volundd process-next-preview
```

The first command is idempotent for a content hash, profile, converter version,
and contract version. The worker verifies the original SHA-256 again, invokes
the guarded native runner, and atomically records `preview.glb`, `assembly.json`,
`diagnostics.json`, and `result.json`. Production polls the queue through the
native `volund-preview-worker.timer`.

Ready artifact metadata contains an API URL. GLB and JSON responses stream from
disk, support a single HTTP byte range, and use their SHA-256 as an immutable
ETag:

```sh
curl -H 'Range: bytes=0-1048575' \
  http://127.0.0.1:8080/api/v1/previews/<preview-id>/artifacts/preview-glb
```

## Conversion jobs

The first `volundd` capability is a native, synchronous conversion job runner:

```sh
volundd convert \
  --input /srv/volund/library/printer.step \
  --profile web
```

The input may be STEP, IGES, BREP, STL, 3MF, OBJ, PLY, glTF, or GLB. Format
selection and validation are delegated to the isolated native worker.

Defaults are intentionally conservative:

- one active OCCT converter per LXC, enforced by an advisory `flock` lock;
- a 30-minute per-job timeout using GNU `timeout`;
- adaptive `web` preview quality unless `--profile fine` is requested;
- immutable job publication by renaming a completed work directory.

Jobs move through these filesystem states:

```text
/srv/volund/derived/.work/<job-id>/   queued or running
/srv/volund/derived/jobs/<job-id>/    completed successfully
/srv/volund/derived/failed/<job-id>/  failed or timed out
```

Every directory contains an atomically updated `job.json` conforming to
[`contracts/job-v1.schema.json`](../../contracts/job-v1.schema.json). Successful
jobs also contain the five converter artifacts. Failed jobs retain diagnostics
and partial artifacts for inspection instead of presenting them as ready.

The global lock is `/srv/volund/scratch/cad-convert.lock`. `flock` releases it
automatically if `volundd` or the host terminates, so no stale lock recovery is
needed. Time spent waiting for the lock does not consume the conversion timeout.

The runner does not claim to be the final persistent queue. PostgreSQL-backed
submission and restart recovery will be added with the API. This layer already
provides the process isolation, state layout, timeout behavior, and single-job
invariant that the later queue will reuse.
