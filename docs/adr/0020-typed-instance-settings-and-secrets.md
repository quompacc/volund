# ADR 0020: Typed instance settings, precedence, and secrets

- Status: Accepted
- Date: 2026-08-29

## Context

Runtime behavior is currently selected by constants, command options, and
environment variables. A generic key/value settings table would make unsafe
values easy to invent, obscure where an effective value came from, and tempt
ordinary APIs to return credentials.

## Decision

VÖLUND has a versioned registry of typed setting definitions. Each definition
owns its key, domain, type, default, validation, required capability, mutability,
restart or migration effect, and whether it is sensitive. Unknown keys are
rejected. Persisted non-secret values are stored as validated JSON together with
revision, actor, and timestamps.

Effective values use this precedence:

1. enforced environment or file-based operator override;
2. validated persisted instance value;
3. compiled safe default.

The settings API returns the effective value, origin, editability, validation
constraints, current revision, and restart/migration effect. Updates use
optimistic revision checks and an atomic audit event. An environment-enforced
value is visible as non-editable; submitting a replacement is rejected rather
than stored ineffectively. Validation occurs before persistence.

Secrets are never returned by ordinary settings APIs and are not stored in the
general settings table. Secret definitions expose only `configured`, origin,
and rotation/restart metadata. Secret material comes from a root-controlled file
or process environment and is read through a dedicated interface. Diagnostics
mask credentials in URLs and command output.

The first registry covers instance name, locale, time zone, session durations,
import defaults, preview profile, thumbnail policy, and the operational limits
already enforced by Phase 1. Retention, job concurrency, capacity, and worker
controls belong to Phase 2, where their runtime owners and health feedback are
implemented. Filesystem roots, database connection, listener, trusted
proxy, TLS cookie mode, and bootstrap secret remain operator-enforced until a
safe privileged reconfiguration workflow exists.

Settings whose new values require restart are persisted as pending and do not
pretend to be effective. The API reports both active and pending values. A later
administration workflow may validate and perform the restart; Phase 1 only makes
the state truthful.

## Consequences

- Administrators can explain both a value and its source.
- Configuration cannot silently drift through arbitrary database keys.
- Secret handling stays separate from ordinary backup/export and API paths.
- Adding a setting requires code, validation, API schema, and tests rather than
  an unreviewed database insert.
