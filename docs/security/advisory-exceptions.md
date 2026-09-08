# Dependency advisory exceptions

Exceptions are narrow, time-bounded decisions for advisories that affect the
lockfile but are not reachable in a shipped VÖLUND build. They do not suppress
other RustSec findings.

## RUSTSEC-2023-0071

- Component: `rsa 0.9.10`
- Upstream path: inactive `sqlx-mysql` lockfile packages
- Shipped configuration: SQLx uses `default-features = false` with only
  `runtime-tokio`, `postgres`, `migrate`, and `macros`
- Verification: `cargo tree -i rsa` returns no active dependency for the
  VÖLUND build target
- Product exposure: no RSA private-key operation was identified
- CI handling: `cargo audit --ignore RUSTSEC-2023-0071`; every other advisory
  still fails the job
- Owner: VÖLUND maintainers
- Recorded: 2026-08-29
- Review deadline: 2026-11-29 or the next SQLx update, whichever comes first
- Last reviewed: 2026-09-06 with `cargo-audit 0.22.2`, advisory database
  commit `5a0ebedfe8bdd2e295b171f4162f8c977bcad9a5`; both the normal and
  all-targets feature trees contain no active `rsa` dependency

Remove the exception if the inactive MySQL lockfile branch disappears. Escalate
it immediately if `rsa` becomes active, SQLx features change, or the advisory's
affected scope expands.
