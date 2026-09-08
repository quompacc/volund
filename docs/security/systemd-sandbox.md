# systemd sandbox baseline

Status: Verified templates and installed v0.36.0 units
Last verified: 2026-08-30 on Debian 13

VÖLUND uses separate sandboxes for the HTTP daemon and native preview worker.
The daemon needs Unix, IPv4, and IPv6 sockets for PostgreSQL and its loopback
listener. The worker needs only Unix sockets for local PostgreSQL and converter
process coordination. Neither service needs Linux capabilities, hardware
devices, kernel administration, namespace creation, realtime scheduling, or
access to other processes.

The version-controlled units enforce those boundaries with empty capability
sets, restricted address families, private devices and temporary directories,
read-only operating-system and home trees, protected kernel and control-group
interfaces, a PID-only `/proc`, native system-call architecture, and blocked
SUID/SGID creation. Their storage allowlists remain deliberately different:

- `volundd` may write the authoritative library, incoming staging area, and
  private transient support staging directory and may only read derived
  artifacts and web assets.
- `volund-preview-worker` may only read originals and may write derived and
  scratch data.

On Debian 13, `systemd-analyze verify` accepts both templates. Offline
`systemd-analyze security` reports exposure 2.9 for `volundd` and 2.6 for the
worker, compared with 8.3 for both audited production units.

The v0.36.0 templates are installed in production. `systemd-analyze verify`
accepts the installed daemon, worker, scheduler, retention, and backup units;
the loopback API, catalog state, worker heartbeats, support staging, and service
restart behavior passed production acceptance with zero automatic restarts.

P2.5 does not grant `volundd` journal or systemd control access. Recent support
events are read from a narrow sanitized PostgreSQL table through the existing
peer-authenticated runtime identity. `/srv/volund/support` is mode 0750, owned
by `volund`, contains only fixed-name temporary archives, and is empty after
successful or failed generation.

System-call filters and `MemoryDenyWriteExecute` remain deferred until native
OpenCascade and Assimp behavior is traced under representative conversions.
`RemoveIPC` is also deferred because daemon and worker currently share the
`volund` Unix account. These controls must not be added merely to improve a
numeric exposure score.
