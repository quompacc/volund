# ADR 0024: Owned import lifecycle and explicit publication target

- Status: Accepted
- Date: 2026-08-30

## Decision

Every import draft belongs to the creating user. Editors can list and mutate
only their own drafts; administrators and owners can inspect all and perform
confirmed cancellation/cleanup. Viewers have no import access. Ownership is
checked in both route and domain queries. Security-relevant success and denial
events contain stable IDs and codes, never paths, submitted confirmations, or
raw errors.

The persisted state machine is `draft`, `uploading`, `uploaded`, `review_ready`,
`reviewed`, `committing`, `committed`, `failed`, `cancelled`, and `expired`.
Database constraints restrict values; row-locked domain transitions restrict
edges. Cancellation may win until `committing`; a locked commit then runs to a
terminal result. Completed commit is idempotently returned from persisted result
fields. Draft retention never deletes committed history.

Publication target is explicit. `create` refuses a colliding slug. `update`
requires a stable model public ID and expected revision; commit locks and
rechecks that revision. Slug similarity is only a suggestion. Existing links
are additive, and removal/replacement is outside Phase 4.

The native `volund-import-cleanup` oneshot/timer owns expiry and staging cleanup,
heartbeat, structured logs, and health. It uses PostgreSQL row locking and a
canonical, symlink-free incoming root. Legacy ownerless active drafts with no
staged bytes are retained as expired with `legacy_draft_expired`.

## Consequences

Reload, session change by the same actor, and daemon restart are safe. Resource
boundaries are explicit, foreign drafts do not leak filenames or metadata, and
implicit model updates are eliminated. v0.36.1 binaries ignore additive columns
and terminal statuses but must not be used to mutate Phase 4 drafts after a
rollback; rollback therefore stops intake or restores the pre-deployment dump.

