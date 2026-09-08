# ADR 0006: Versioned read-only HTTP API

- Status: Accepted
- Date: 2026-08-28

## Context

The browser client and future integrations need a stable boundary around the
PostgreSQL catalog. Direct database access would expose deployment details,
couple clients to migrations, and make later authorization difficult.

## Decision

VÖLUND exposes an Axum-based JSON API below `/api/v1`. The first contract is
strictly read-only and covers health, library roots, source files, content
identities, and scan history. It is documented in
`contracts/http-api-v1.openapi.yaml`.

Responses use public UUIDs and relative library paths. Absolute filesystem paths
and raw database diagnostics are never returned. File and scan collections use
deterministic ordering and bounded offset pagination with a maximum page size of
200. Missing files are excluded unless requested explicitly.

The server binds to `127.0.0.1:8080` by default. Network exposure must be an
explicit deployment choice through `VOLUND_LISTEN_ADDR` or a reverse proxy. The
`serve` command checks schema health but never applies migrations.

## Consequences

- Clients depend on a versioned application contract rather than the schema.
- A reverse proxy can later add TLS and authentication without changing catalog
  queries.
- Host mount paths and database errors remain private.
- Mutating workflows require a separately designed authenticated API contract.
- Offset pagination is simple and deterministic for the initial catalog; very
  large interactive result sets may later justify cursor pagination.
