# ADR 0014: Streamed import staging

- Status: Accepted
- Date: 2026-08-28

## Context

Project imports can contain hundreds of files and individual CAD assemblies of
several hundred megabytes. Buffering an upload in application memory or writing
directly into the authoritative library would risk exhausting the service and
exposing incomplete files to the scanner.

## Decision

VÖLUND streams each reviewed draft item into `/srv/volund/incoming`, a dedicated
directory on the data mount. Staged filenames are server-owned UUIDs and never
derive from browser paths. The API accepts content only for an item belonging to
the addressed active draft and requires its HTTP content length to match the
previously reviewed manifest.

Each upload is written with create-new semantics to a `.part` file while its
byte count and SHA-256 are calculated. Exceeding or missing the declared byte
count fails the request and removes only that exact temporary file. A successful
upload is atomically renamed to `.bin` and recorded in PostgreSQL. Already
completed items are idempotent; concurrent writes to one item are refused.

Staging does not create a model, move a library original, or expose a file to
the scanner. Content deduplication, final-path conflict handling, and the atomic
library commit remain a distinct confirmation step.

## Consequences

- Large files never need to reside fully in API memory.
- Interrupted imports can retain completed items and retry incomplete ones.
- The service gains write access to the incoming directory, but derived output
  remains read-only and authoritative files change only during explicit commit.
- Staged data needs a later retention and cancellation policy.
