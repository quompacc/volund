# ADR 0009: Native-hosted TypeScript viewer

- Status: Accepted
- Date: 2026-08-28

## Context

VÖLUND needs a useful browser interface for large, heterogeneous CAD libraries
and its generated GLB previews. The native Debian deployment must remain simple
and must not acquire a container or JavaScript runtime dependency.

## Decision

The browser application uses strict, framework-free TypeScript, Vite, and
Three.js. Avoiding a component framework keeps the initial state model and
dependency surface small while the product workflows are still evolving.

Vite and Node.js are build-time tools only. The generated static files are
installed below `/usr/share/volund/web` and served by `volundd` from the same
origin as `/api/v1`. Unknown API routes retain structured JSON errors; browser
routes fall back to `index.html`. The daemon adds a restrictive content security
policy plus frame and MIME-sniffing protection.

The first interface is deliberately read-only. It browses library roots and
files, displays conversion state, and renders ready GLB artifacts. Mutating API
operations, authentication, and public network exposure require separate threat
models and later decisions.

## Consequences

- Production needs one process and no Node.js, nginx, or container runtime.
- Browser/API requests are same-origin and need no permissive CORS policy.
- Three.js remains isolated behind a small viewer class.
- Frontend artifacts are reproducibly built from `package-lock.json` but are not
  committed as generated output.
