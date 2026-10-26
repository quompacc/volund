# ADR 0037: Personal API tokens

- Status: Accepted
- Date: 2026-10-26

## Context

Scripts, backup jobs, and slicer or CAD tool integrations could use the API only
through a browser session: log in with a password, keep the cookie, and send the
CSRF header. That puts passwords into scripts and gives every script the full
power of its user. The product contract requires API tokens that are "hashed at
rest, scoped, expiring where practical, and shown only once".

## Decision

- **Format.** A token is `vlnd_` followed by 64 hex digits from `OsRng`
  (256 bits). The database stores only its SHA-256 digest, plus the first eight
  hex digits so a token can be recognized in lists. The secret appears once,
  in the creation response.
- **Ownership and lifetime.** Each token belongs to one account. It always
  expires: 1 to 365 days, 90 by default. An account holds at most 20 active
  tokens.
- **Revocation.** Revoking a token, disabling its account, or reaching the
  expiry date stops it at once. `last_used_at` records use.
- **Scope.**
  - `read` grants catalog read access.
  - `catalog-write` grants catalog read and write access. Only accounts that
    may write the catalog can create it.
  - The token's capabilities are always intersected with the account's
    current role. A token never carries user, settings, library, or metadata
    administration.
- **Authentication.** `Authorization: Bearer vlnd_…` is checked before the
  session cookie. A request that presents a bearer header is judged by that
  header alone: a malformed or unknown token is 401, with no fallback to the
  cookie.
- **Deny by default.** For token requests, the middleware resolves the
  declared route policy of the full request path before any handler runs.
  Routes without a declaration, public routes, and the browser-only areas
  (account, session(s), users, setup, invitations) are refused with 403 and an
  audit event. Tokens therefore cannot create tokens, change passwords, or
  manage sessions.
- **No CSRF for bearer requests.** Bearer tokens are not ambient credentials,
  so CSRF and origin checks do not apply to them. Browser sessions keep both
  checks unchanged.
- **API and audit.** `/api/v1/account/tokens` lists, creates, and revokes the
  signed-in account's tokens. Creation and revocation are audited. The browser
  shows tokens under Konto → API-Tokens: a one-time secret with a copy button,
  and revocation behind a confirmation.

## Consequences

- Automation needs no passwords. A leaked token is limited in scope and
  lifetime and can be revoked on its own.
- Service accounts are ordinary accounts with tokens. Finer resource grants
  (per library or collection) remain future work.
