# ADR 0026: Commit catalog state before unlinking source bytes

- Status: Accepted
- Date: 2026-09-05

## Context

A PostgreSQL rollback cannot undo filesystem unlink operations. Quarantine and
recovery used to remove the catalog-owned link before audit/commit, and purge
removed the last link before persisting its result. Compensating hard links
also risked removing the remaining bytes when restoration failed.

## Decision

Stage quarantine/recovery using a no-overwrite hard link, verify the bytes,
and sync the new file and its parent directory before committing catalog state.
The old catalog-owned link remains intact. Audit, revision, plan consumption
and a `source_cleanup` intent commit in the same transaction. Purge only records
an unlink intent until that transaction commits.

After commit, finish the intent under the source row lock: validate confined
paths, regular-file metadata, byte count and hash; quarantine/recovery also
require a surviving keeper with identical inode/device. Unlink the obsolete
path, sync its parent, then delete the intent. Missing obsolete paths make a
retry idempotent if the previous unlink succeeded but its SQL acknowledgement
failed. A scheduler run retries bounded batches, and a subsequent lifecycle
transition finishes any pending intent for its source first.

Before COMMIT starts, a dropped operation removes only its newly staged link,
and only while the original link still names the same regular file. Once COMMIT
starts, its outcome may be ambiguous: keep both links instead of guessing.
An operation retry may adopt an existing destination only when it is the same
inode/device, never merely because it has equal bytes.

## Consequences and boundaries

- Migration 0031 is additive; old migrations remain immutable. Readiness expects
  31 migrations and 37 schema tables. Deploy the matching binary with migration.
- Failure after catalog commit reports pending cleanup rather than success;
  the scheduler retries it. Until then an extra link may remain on disk.
- A process crash before commit can leave an uncommitted extra hard link. The
  catalog-owned copy remains valid; repeating the operation adopts that link.
  Unknown/orphan links are not automatically purged. This favors byte safety
  over reclaiming every possible orphan.
- Unavailable storage, replaced paths or changed bytes retain the cleanup intent
  and cause an unhealthy scheduler result without suppressing unrelated jobs.
- This is not isolation from a hostile process concurrently modifying the same
  library through the operating system. Deployment must retain exclusive control
  of the managed quarantine directory. Fault injection covers SQL/commit errors
  and interrupted cleanup, not a physical power-loss certification.

## Import application

Reviewed imports follow the same commit-before-unlink rule for relocations.
Matched source rows are locked in ID order before reading their paths. Relocation
cleanup uses the existing migration 0031 intent and scheduler. Newly copied
files retain a temporary hard-link anchor until the operation completes; dropping
an uncommitted operation removes its staged destination before its anchor.

A cancelled future before commit is covered by regression tests for both new
files and relocations. A hard process crash or ambiguous commit can still leave
extra links or temporary files. New-copy leftovers are not automatically adopted
by a retry; reconciliation may be required. This is not a power-loss guarantee.

Import plans may relocate each source ID at most once. Further same-content
uploads are planned as new copies, retaining separate filenames and source IDs.
Commit rejects legacy duplicate-relocation plans before staging. Pending cleanup
from earlier operations is finished in a separate pre-staging pass; no cleanup
intent created by the current import may run before its commit. Internal
quarantine destinations are rejected both at resolution and publication.

## Managed move application

Ordinary managed moves also stage a guarded hard link and commit the path,
move audit and cleanup intent together before unlinking the old path. A new
move finishes an existing cleanup intent under the source row lock first.
The source hash must still match the catalog; changed bytes require a new scan.
This adds a full-file read before moving. The internal root-level quarantine
directory and its descendants are not valid destinations for ordinary moves.
The same ambiguous-commit and hard-crash boundaries apply as above.

## Credential concurrency

Session creation and own-password change lock the user, then read/lock the
current credential in a separate statement. The verified hash and usable account
state must still match. Admin reset already follows user-before-credential order.
This keeps expensive Argon2 work outside locks while preventing an old verified
password from creating a session or overwriting a newer credential after reset.
