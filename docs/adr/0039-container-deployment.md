# ADR 0039: Container deployment with Docker Compose

- Status: Accepted
- Date: 2026-11-05
- Supersedes: [ADR 0003](0003-native-linux-deployment.md)

## Context

The operator's homelab moved from Proxmox LXCs to a single Debian 13 host on
which every service runs as a Docker Compose stack behind one central Caddy
proxy. A dedicated VÖLUND LXC no longer exists, and installing PostgreSQL,
OpenCascade, and systemd units directly on the shared host would mix VÖLUND's
runtime with unrelated services. The operator decided to run VÖLUND as
containers on that host.

ADR 0003 forbade Docker as an application runtime. Its reasons remain valid:
system-package OCCT and PostgreSQL, conventional paths, an unprivileged service
account, and strict per-process isolation. This ADR keeps them inside images.

## Decision

- **Images.** `deploy/docker/Dockerfile` builds two images from Debian 13
  packages:
  - `app`: `volundd`, `volund-cad-convert`, and the static web build, at the
    native paths (`/usr/local/bin`, `/usr/share/volund/web`, `/srv/volund`)
    and run as the `volund` account (uid/gid 10010). The runtime installs
    exactly the Debian packages that provide the binaries' shared libraries.
  - `db`: `postgres:17` plus a `volund` OS account with the same uid, so that
    peer authentication keeps working without a database password.
- **Processes.** `deploy/docker/compose.yaml` maps each systemd unit to one
  container:
  - `volundd serve`;
  - the preview, scan, scheduler, import-cleanup, retention, and backup jobs;
  - a one-shot `migrate` + `database-doctor` gate that every application
    container waits for.

  Workers run under `volund-loop`, which repeats a command with a pause after
  each run (the `OnUnitInactiveSec` semantics). It forwards `SIGTERM` to the
  running job so that the lease recovery of ADR 0030 is unchanged.
- **Isolation.** The systemd sandbox maps to container settings:
  - Every container has a read-only root filesystem, drops all capabilities,
    and sets `no-new-privileges`.
  - Memory and task limits are taken from the units.
  - Mounts mirror `ReadOnlyPaths`/`ReadWritePaths`: workers see the library
    read-only, and the daemon sees derived data read-only.
  - The database and all workers have no network (`network_mode: none`), the
    equivalent of `RestrictAddressFamilies=AF_UNIX`. They reach PostgreSQL
    only through a shared in-memory socket directory. PostgreSQL does not
    listen on TCP.
  - Only the daemon joins the reverse-proxy network, and no port is published
    on the host. This replaces the nftables proxy rule for this deployment.
- **Data.** Host bind mounts keep the native layout:
  - authoritative libraries are configured separately from rebuildable
    derived, scratch, and database data;
  - validated dumps go to a backup directory on a different device.
- **Native deployment remains documented.** `deploy/debian/README.md` and the
  systemd examples stay valid for dedicated hosts. Both paths build the same
  sources with the same lockfiles.

## Consequences

- Upgrades replace images instead of binaries. The migration and rollback
  rules of `INSTALLATION.md` still apply. Keep the previous image tags as
  rollback artifacts.
- The systemd sandbox rehearsal no longer covers the container deployment.
  Container isolation is reviewed through `compose.yaml`.
- `deploy/docker/tests/volund-loop-test.sh` covers the timer replacement in CI.
