# ADR 0042: Stralar slicer service behind VÖLUND

- Status: Accepted
- Date: 2026-11-07

## Context

Stralar is the operator's own slicer: a browser studio, a local bridge that runs the
slicing engine and talks to the printers, and per-user workspaces. It shall be usable from
every device in the home network — PC, tablet, phone — with VÖLUND's accounts, so that it
needs no user management of its own (Stralar `ARCHITECTURE.md` §8). Both projects stay
separate repositories and services; neither is merged into the other.

Stralar's bridge is a Node service. VÖLUND's rule "no Node at runtime" concerns VÖLUND
itself; Stralar runs as a separate service on the same host and listens on loopback only.
The browser must reach it through VÖLUND, under VÖLUND's origin, without exposing VÖLUND's
credentials to it and without letting a client choose its identity there.

## Decision

- **Route.** `/stralar/*` is a reverse proxy to `VOLUND_STRALAR_URL`, which must be plain
  HTTP on a loopback address (`127.0.0.1`, `[::1]`, `localhost`) with an explicit port.
  `/stralar` redirects to `/stralar/` so the studio's relative URLs resolve below the
  prefix. Without `VOLUND_STRALAR_URL` the route does not exist (404). A set but invalid
  URL, or a secret shorter than 32 characters, yields 503 `stralar_misconfigured`, and the
  daemon logs the reason once at startup.
- **Authentication.** Only an active browser session reaches Stralar (`CatalogRead`, no
  pending password change). A request with an `Authorization` header is refused with 403:
  bearer tokens never reach Stralar, consistent with the browser-only areas of ADR 0037.
- **Cross-site protection.** Stralar's browser code does not carry VÖLUND's CSRF token.
  Every mutation through the proxy therefore needs a **present** `Origin` equal to the
  request's own scheme and host (the API's origin check also accepts an absent `Origin`).
  Together with the `SameSite=Lax` session cookie this blocks cross-site mutations. A
  mismatch is answered like a CSRF failure and recorded as a security audit event.
- **Identity hand-off.** VÖLUND removes `Cookie`, `Authorization`, `X-CSRF-Token`, every
  client-supplied `x-stralar-*` header and hop-by-hop headers, then adds
  `x-stralar-user` (the account's public id) and `x-stralar-proxy-secret` (from the file
  named by `VOLUND_STRALAR_SECRET_FILE`). Stralar accepts an identity only together with the
  secret, so other local processes cannot impersonate an account.
- **Transport.** Request and response bodies stream (model uploads, live printer status via
  server-sent events, the camera stream). The connect timeout is five seconds; there is no
  total timeout, because status and camera streams stay open. An unreachable Stralar yields
  503 `stralar_unavailable`. Responses keep Stralar's own `Content-Security-Policy`
  (WebAssembly, workers and inline styles are needed by the studio); VÖLUND's default
  headers are added only where Stralar sets none (`response_headers`).
- **Logging.** Each proxied request is recorded in the operational request log like an API
  request.

## Consequences

- One login, one origin and one certificate for VÖLUND and Stralar; Stralar keeps working
  standalone on a workstation without VÖLUND.
- The native deployment configures the integration in `/etc/volund/volund.env`. The
  container deployment cannot reach a loopback service on the host; it stays disabled there
  until Stralar ships its own container (Stralar block S5), which will add an internal
  upstream for compose.
- Within Stralar every account gets its own workspace; printer access per account follows
  in Stralar (block S6).
- The `/stralar/` prefix is reserved; the web application must not use it for own routes.

## Addendum (0.43.0): opening files in Stralar

- `GET /api/v1/stralar` (authenticated) returns `{"available": bool}`; a misconfigured
  integration reports `false`. The web application shows Stralar entries only when it is
  `true`.
- The navigation entry "Stralar" opens `/stralar/` in a new tab.
- "In Stralar öffnen" in the file view links to
  `/stralar/#/open?file=/api/v1/files/{id}/content&name={file name}`. The request sits in the
  URL fragment, so it never reaches a server log. Because Stralar runs under the same origin,
  it fetches the download itself with the browser session; no bearer link or token is
  created. Stralar accepts only same-origin paths (`/…`, never `//…`) in this form and asks
  the user before importing; an opened link alone never changes data.

## Addendum (0.44.0): container deployment

- Stralar ships a container image (Stralar `deploy/docker/Dockerfile`, block S5). VÖLUND's
  compose stack includes it through the optional file `deploy/docker/compose.stralar.yaml`.
- `VOLUND_STRALAR_URL` now also accepts `http://SERVICE:PORT`, where `SERVICE` is a single
  lowercase DNS label (`stralar`). Such a name has no dots, so it is no public host name; Docker
  resolves it on the compose network. IP addresses other than loopback, dotted names, HTTPS
  and paths stay refused, as before.
- Network boundary: VÖLUND and Stralar share the `internal` network `stralar` (no route out, no
  host port). Stralar additionally joins `stralar-lan` to reach the printers; VÖLUND does not.
  Because the container listens on all its interfaces, Stralar enforces the shared secret on
  every request (it only does so in server mode and refuses a non-loopback bind otherwise).
- Secret: one file `secrets/stralar-proxy`, owner `10010` (VÖLUND), group `10020` (Stralar), mode
  `0440`, mounted into both containers as a compose secret.
- The native deployment is unchanged: Stralar's systemd unit (`deploy/systemd/stralar.service`
  in Stralar) reads `/etc/volund/stralar.secret` as a systemd credential and listens on loopback.
