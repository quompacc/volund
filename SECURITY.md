# Security policy

VÖLUND is a pre-1.0 alpha intended for a single self-hosted instance and a small
trusted team. It has not received an independent security review. Do not expose
the backend directly to an untrusted network or operate it as a public
multi-tenant service.

## Maintenance scope

| Version | Security maintenance |
| --- | --- |
| Current `main` / `Unreleased` | Receives fixes and audit coverage; not a released artifact |
| Latest tag (currently 0.38.3) | Eligible for a separately reviewed security-fix release |
| Older tags | No security update commitment |

This table describes repository maintenance. A fix on `main` is not installed
anywhere automatically, and production deployment always requires an explicit
operator decision.

## Reporting a vulnerability

Do not include exploit details, credentials, private CAD data, internal
hostnames, or addresses in a public issue. Report the problem privately to the
repository owner through the private contact channel by which access to this
source was granted. If no private channel is available, ask the owner for one
without disclosing the vulnerability details.

Include the affected commit or tag, component, prerequisites, reproducible
steps using synthetic data, impact, and any known mitigation. Do not test a
report against production or data you do not own.

The maintainer should acknowledge the report privately, reproduce it in an
isolated environment, record affected versions and mitigations, and coordinate
disclosure only after a fix and release decision. No fixed response-time SLA is
promised at the current project maturity.

## Security boundaries

- The supported runtime is native Debian with systemd sandboxing and a
  loopback-bound API. Remote access requires an operator-managed same-origin TLS
  reverse proxy and secure cookies.
- Local users, opaque revocable sessions, CSRF/origin checks, and role policies
  protect application routes. VÖLUND is not a tenant-isolation boundary.
- Authoritative source files remain filesystem-owned. Scanners and converters
  must not modify them; explicit managed moves are audited and constrained to a
  configured library.
- Secrets must be supplied through protected files or deployment configuration,
  never committed to the repository or included in support bundles.
- Known dependency exceptions are listed with scope and expiry in
  [`docs/security/advisory-exceptions.md`](docs/security/advisory-exceptions.md).

Operational details are in [`docs/security/systemd-sandbox.md`](docs/security/systemd-sandbox.md),
[`docs/LOGGING.md`](docs/LOGGING.md), and [`docs/BACKUP.md`](docs/BACKUP.md).
Supported environments and known limitations are listed in
[`SUPPORT.md`](SUPPORT.md).
