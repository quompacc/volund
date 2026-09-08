# ADR 0019: Deny-by-default role authorization

- Status: Accepted
- Date: 2026-08-29

## Context

The current router mixes reads and mutations on one anonymous API surface.
Adding authentication middleware alone would still leave new routes vulnerable
to accidental omission and would not define the owner, administrator, editor,
and viewer behavior promised by the product contract.

## Decision

Authorization uses named capabilities checked by the API, not UI visibility and
not direct comparisons of role strings. The initial instance roles map to
capabilities as follows:

- `owner`: every instance capability, including security policy, owner
  appointment, recovery, storage, backup, and update control;
- `administrator`: user lifecycle except owner appointment/removal, libraries,
  jobs, diagnostics, and non-security instance settings;
- `editor`: catalog mutations, imports, metadata, collections, thumbnails, and
  generated preview requests;
- `viewer`: authenticated catalog, preview, download, search, and export reads.

Routes declare one of four policies when registered: public, authenticated,
capability-required, or internal-only. The route-policy table is exhaustive and
is compared with the Axum router in an automated contract test. A route missing
from that table fails the test and is denied at runtime. `/health/live`, login,
setup status, and guarded first-owner setup are the only initial public routes.

Handlers receive an authenticated actor produced by middleware and still check
the capability at the mutation boundary. Background jobs retain the initiating
actor where one exists. Database audit rows use immutable actor identifiers and
may keep a safe display snapshot after an account is renamed or disabled.

Role changes and account disablement revoke affected sessions in the same
transaction. The last enabled owner cannot be disabled, demoted, or deleted.
Users cannot elevate their own role. Owner-only operations require recent
authentication once that mechanism is implemented.

Resource-scoped grants are deliberately deferred. Capability checks accept an
optional resource context so a later scope model can narrow access without
changing every handler. Until then, authenticated content visibility is
instance-wide.

## Consequences

- New endpoints fail closed unless their policy is deliberately declared.
- The API, rather than the browser, is the security boundary.
- Initial roles remain understandable while capabilities avoid scattering a
  fragile role hierarchy across handlers.
- Public catalog sharing requires a later explicit share/token design and is not
  emulated by anonymous viewer access.
