# Installing VÖLUND

VÖLUND 0.39.0 is installed from source. There is currently no supported binary
package, container image, guided installer, or unattended upgrade mechanism.
The validated runtime target is Debian 13 (`trixie`) on x86-64 with PostgreSQL
17, OpenCascade 7.8, Assimp, and systemd.

The canonical command-level procedure is
[`deploy/debian/README.md`](deploy/debian/README.md). Read this overview and the
linked procedure completely before changing a host.

## Required build environments

- Rust 1.85 or newer for the workspace and daemon.
- CMake 3.25 or newer, a C++ compiler, OpenCascade 7.8, Assimp, TBB, and
  Fontconfig for the native converter.
- Node.js 20.19 or newer for building and testing the static browser assets.
  Node.js is not required on the runtime host after those assets are built.
- PostgreSQL 17 with UTF-8 encoding and data checksums for the validated
  production-shaped database configuration.

Use the exact dependency lockfiles. Run `cargo` with `--locked` and install web
dependencies with `npm ci`.

## Installation sequence

1. Select a reviewed commit or annotated release tag. Build and test the Rust
   workspace, web assets, and native converter on the supported Debian target.
2. Create the unprivileged `volund` service identity and the persistent paths
   described in the Debian procedure. Keep authoritative libraries separate
   from rebuildable derived and scratch data.
3. Create the PostgreSQL role/database using local peer authentication. Do not
   place a production database password in the environment when peer
   authentication is used.
4. Install `volundd`, `volund-cad-convert`, and the root-owned static web build.
   Install only the required example systemd units after reviewing their paths,
   limits, and sandbox allowlists for the host.
5. Configure `/etc/volund/volund.env`. Keep the API on `127.0.0.1:8080` unless a
   deliberate firewall-protected proxy path has been prepared. HTTPS access
   requires `VOLUND_SECURE_COOKIES=true` and a same-origin reverse proxy.
6. Run `volundd migrate`, followed by `volundd database-doctor`, as the service
   identity before starting the daemon.
7. Start the daemon and required worker timers, then verify API health, static
   assets, unit status, logs, database state, converter version, and filesystem
   permissions.
8. Bootstrap the first owner with a short-lived root-owned token as described in
   the Debian procedure, then remove the token and its environment entry.

## Lizenztexte mitnehmen

VÖLUND steht unter AGPL-3.0-only. Bei Weitergabe LICENSE,
THIRD_PARTY_NOTICES.md und docs/licensing zusammen mit den Quellen erhalten.
Der Webbuild enthält LICENSE.txt und THIRD_PARTY_NOTICES.txt; beide Dateien
mit dem vollständigen statischen Baum installieren. Vor öffentlicher
Bereitstellung außerdem das zum ausgelieferten Stand passende Quellcodeangebot
gemäß [Lizenzhinweisen](docs/licensing/README.md) prüfen. Die Debian-
Systembibliotheken behalten ihre eigenen Lizenz- und Copyrightdateien.

## Upgrade and rollback

Migrations are immutable and forward-only. Take and verify a database backup
before any upgrade, retain the previous binaries and web tree, and never assume
that rolling back binaries can reverse an applied database migration.

The 0.39.0 candidate was rehearsed against a verified copy of
the actually installed v0.38.3 database. Its two migrations are atomic and
preserve catalog data, but v0.38.3 deliberately refuses the resulting newer
schema. Keep all application writers stopped from the final pre-upgrade dump
until the acceptance decision. A rollback after either users or workers have
written to the upgraded database requires a forward fix or explicit data
reconciliation; restoring the old dump would otherwise discard those writes.

The command-level freeze, upgrade, validation, and full database rollback are
documented in [`deploy/debian/README.md`](deploy/debian/README.md). The isolated
rehearsal is reproducible with
[`deploy/debian/tests/upgrade-rehearsal.py`](deploy/debian/tests/upgrade-rehearsal.py).
Fresh host provisioning and upgrades were rehearsed on isolated Debian 13
systems. Production acceptance remains operator-specific.

Do not treat these source instructions as deployment approval. Release creation
and production deployment require separate operator authorization.
