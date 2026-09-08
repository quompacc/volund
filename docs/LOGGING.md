# Structured logging and support bundles

VÖLUND's native daemon, scan worker, preview worker, scheduler, retention
worker, and backup service emit one compact JSON object per journal event.
Every event contains a UTC RFC 3339 timestamp, severity, stable event name,
component, stable code, product version, and bounded sanitized message. HTTP,
job, and run correlation IDs are present where the runtime owns one.

## Journal operation

systemd captures standard output and error; VÖLUND does not manage a second
root-owned log file and the daemon has no journal-reading permission. Inspect
events as an authorized host operator:

```sh
journalctl -u volundd.service -o cat --since today
journalctl -u 'volund-*.service' -o cat --since today
journalctl -u volund-backup.service -o cat --since today
```

journald owns persistence, rotation, compression, disk ceilings, and vacuuming.
Use the Debian host's central `journald.conf` policy; do not grant the `volund`
account membership in `systemd-journal` and do not make the journal readable by
the HTTP service. The application stores only a narrow recent operational event
view in PostgreSQL for support bundles. Support downloads select at most 500
events from the last 24 hours.

## Disclosure boundary

Messages are limited to 512 UTF-8 bytes. Passwords, tokens, cookies,
authorization headers, CSRF/bootstrap secrets, private-key markers, database
URLs, environment dumps, command lines, raw child output, and complete private
host paths are replaced before journal or database publication. Stable codes,
not raw operating-system or database failures, are the operator contract.

Request logging records the method, normalized route, status, and bounded
duration. It never records headers, query values, request bodies, response
bodies, or session material. Submitted confirmation text is never audited or
logged.

## Support package

Owners and administrators create a support package from Administration. The
POST endpoint retains ordinary session, CSRF, and same-origin enforcement. It
creates a private temporary TAR below `/srv/volund/support`, publishes it
atomically, reads it into the bounded response, and removes both partial and
published files before returning.

The 4 MiB archive contains only fixed JSON members for build identity,
database/migration counts, redacted health, aggregate job/policy status,
sanitized effective settings, bounded operational events, and `manifest.json`.
The manifest records the VÖLUND version, creation time, member categories,
sizes, and SHA-256 checksum of every other member. Package generation has a
15-second deadline, accepts no caller path, refuses symlinked/aliased roots,
uses create-new files, and is actor-audited.

Verify a downloaded package without trusting its filenames:

```sh
mkdir /var/tmp/volund-support-inspect
tar -xf volund-support-*.tar -C /var/tmp/volund-support-inspect
python3 -m json.tool /var/tmp/volund-support-inspect/manifest.json
```

Remove the inspection directory after review. Support packages do not contain
CAD originals, derived artifacts, raw database dumps, user content, full host
paths, credentials, sessions, invitations, tokens, keys, or raw journal text.
