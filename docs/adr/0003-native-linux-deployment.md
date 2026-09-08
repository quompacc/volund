# ADR 0003: Native Linux deployment without containers

- Status: Accepted
- Date: 2026-08-28

## Decision

VÖLUND targets a native, unprivileged Linux LXC. PostgreSQL and OpenCascade come
from system packages where practical. Rust binaries, the C++ converter, and the
compiled web assets are installed under conventional filesystem locations and run
as dedicated systemd services.

The PostgreSQL data directory and conversion scratch space use local storage. CAD
source roots may be NAS mounts and are initially mounted read-only.

No Docker or nested-container runtime is part of the supported deployment.

The separate Gitea CI runner LXC may use Docker for ephemeral builds and tests;
this exception is isolated from VÖLUND development and production systems.
