# ADR 0008: Immutable derived-artifact streaming

- Status: Accepted
- Date: 2026-08-28

## Context

The browser viewer needs GLB and JSON conversion outputs without direct access
to `/srv/volund/derived`. Preview files can be large, while their database rows
already contain immutable size, media type, and SHA-256 metadata.

## Decision

Ready artifacts are streamed through
`/api/v1/previews/{previewId}/artifacts/{kind}`. The handler resolves only paths
recorded for ready conversions, canonicalizes both the configured derived root
and artifact, and rejects any result outside that root. The opened file size
must match catalog metadata.

Responses stream from disk with bounded memory and support exactly one HTTP byte
range, including open-ended and suffix ranges. Multiple or unsatisfiable ranges
return 416. The artifact SHA-256 is a strong ETag; matching `If-None-Match`
requests return 304. Ready artifacts use immutable cache headers and their
persisted media type.

The API systemd unit sees both originals and derived storage read-only. Only the
separate preview worker retains derived-storage write access.

## Consequences

- Three.js and other clients can consume previews without filesystem knowledge.
- Large GLBs do not need to be buffered in daemon memory.
- Reverse proxies and browsers can cache content safely by its digest.
- Range support is intentionally single-range; multipart ranges are rejected.
- Artifact corruption that changes file size fails closed. A future scrubber can
  periodically verify full artifact hashes without penalizing every request.
