# Backup and recovery baseline

VÖLUND's minimum automated recovery control is a daily PostgreSQL custom-format
dump with an adjacent SHA-256 manifest. The backup script validates the archive
with `pg_restore --list` before atomically publishing either file.

## Reference layout

- Authoritative data mount: `/srv/volund`
- Metadata backup directory: `/var/backups/volund` on a separate device
- Schedule: daily at 03:15 with up to 15 minutes randomized delay
- Retention: 14 days
- Identity: unprivileged `volund`, using PostgreSQL peer authentication

Actual devices, off-host targets and retention requirements are operator
decisions. Do not copy example device names from a different installation.

The script compares the mounted source devices and refuses to run when backup
and authoritative storage resolve to the same device. It also refuses a
symlinked backup directory, validates retention input, writes mode-0600 files,
and leaves no published dump when archive validation fails.

Install the script executable for its unprivileged systemd identity; mode
`0750` with `root:root` ownership prevents `User=volund` from executing it:

```sh
sudo install -o root -g root -m 0755 deploy/backup/volund-backup \
  /usr/local/sbin/volund-backup
sudo install -o root -g root -m 0644 \
  deploy/systemd/volund-backup.service.example \
  /etc/systemd/system/volund-backup.service
```

Success and failure emit the same bounded JSON event contract as the daemon and
write a sanitized recent operational event after updating the backup heartbeat.
Archive filenames, device names, database URLs, and host paths are not written
to the journal. See [structured logging](LOGGING.md).

## What this protects

This baseline provides current metadata recovery if the `/srv/volund` device is
lost while the LXC root disk survives. It is deliberately more useful than the
old dump stored on `/srv/volund` itself.

This application-level dump does **not** protect against loss of the complete
host or site, and it does not duplicate CAD originals. Operators must verify
off-host protection of the database dumps and authoritative CAD volume.

## Verification and restore rehearsal

A release acceptance must restore the scheduled dump into an isolated database,
verify every migration and compare catalog data before the backup is trusted.
This validates the database dump, not clean-host disaster recovery or external
protection of CAD originals.

Inspect the timer and newest result:

```sh
systemctl status volund-backup.timer
journalctl -u volund-backup.service --since today
cd /var/backups/volund
sha256sum -c volund-*.sha256
pg_restore --list "$(ls -1t volund-*.dump | head -n 1)"
```

Restore rehearsals must use a disposable database whose exact name was checked
before creation and deletion. Never restore over `volund`. Record table,
migration, model, source-file, and failed-migration counts before dropping the
disposable database.
