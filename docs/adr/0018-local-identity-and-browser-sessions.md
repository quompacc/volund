# ADR 0018: Local identity and revocable browser sessions

- Status: Accepted
- Date: 2026-08-29

## Context

VÖLUND currently trusts every process that can reach its loopback HTTP listener.
That is sufficient only for the technical prototype. Multi-user operation needs
an application identity, safe first-owner setup, password verification,
revocable sessions, CSRF protection, and security audit events without making an
external identity provider mandatory.

## Decision

Local human accounts are the mandatory baseline. Email addresses are normalized
for lookup but the original display form is retained. Passwords are stored only
as PHC-formatted Argon2id hashes. Parameters are versioned in application policy,
checked on login, and upgraded after a successful verification when policy
changes. Passwords, setup tokens, session tokens, and CSRF tokens are never
logged or stored in plaintext.

An empty database exposes only setup status and first-owner creation. Creation
requires a high-entropy, one-time bootstrap token supplied out of band through a
root-controlled file. The submitted token is hashed before comparison. The
transaction locks instance setup state, verifies that no account exists, creates
exactly one owner, records completion, and invalidates the bootstrap token. A
retry can report the completed state but can never create a second first owner.

Successful login creates separate random 256-bit session and CSRF tokens. Only
SHA-256 digests are persisted. The browser receives an `HttpOnly`, `Path=/`,
`SameSite=Lax` session cookie; TLS deployments use a `Secure` `__Host-` cookie.
Every state-changing browser request also requires the session's CSRF token in
`X-CSRF-Token` and an allowed same-origin `Origin`. Login rotates any existing
session. Sessions have configurable idle and absolute expiry, can be listed and
revoked, and are invalidated immediately when an account is disabled or its
password is reset.

Login responses do not disclose whether an account exists. Authentication is
rate-limited by both normalized account key and client address, with bounded
temporary lockout. Security events record actor, action, outcome, request ID,
safe network metadata, and target identity, never submitted secrets.

Service automation will use separately scoped, revocable credentials in a later
decision. It must not reuse human passwords or browser sessions.

## Consequences

- VÖLUND remains usable without an external identity provider.
- A leaked database does not directly reveal active session or CSRF tokens.
- TLS termination must communicate trusted scheme/origin information through an
  explicitly configured proxy boundary; forwarded headers are not trusted by
  default.
- Recovery needs a documented owner-recovery workflow and cannot be implemented
  by silently bypassing account invariants.
