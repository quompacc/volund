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
sudo install -o root -g root -m 0755 deploy/backup/volund-restore \
  /usr/local/sbin/volund-restore
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
host or site, and it does not duplicate CAD originals. For an exit path that is
readable without VÖLUND or PostgreSQL, use the model packages and the catalog
metadata export described in [EXPORT.md](EXPORT.md). Operators must verify
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

## Restore onto an empty replacement instance

`volund-restore` is the counterpart of the backup script. It restores one dump
into a database that already exists and is empty, and it does nothing else:

- it requires the adjacent `.sha256` manifest and refuses a dump whose checksum
  differs (`restore_checksum_failed`);
- it validates the archive with `pg_restore --list`;
- it refuses a target database that already contains any table, view or
  sequence (`restore_target_not_empty`), so it can never overwrite a live
  catalog;
- it restores in a single transaction with `--exit-on-error`, so a failure
  leaves the target empty;
- it prints one JSON event with the restored migration, model, source-file and
  library-root counts.

The automated rehearsal `apps/volundd/tests/restore_linux.rs` runs this exact
script in CI. It dumps a populated catalog with `pg_dump`, restores it into a
freshly created database, and requires VÖLUND to report that schema as ready
and to serve an identical model catalog.

### Runbook

1. Provision the replacement host as in [INSTALLATION.md](../INSTALLATION.md)
   and [deploy/debian/README.md](../deploy/debian/README.md), including the
   PostgreSQL cluster, role and the empty `volund` database. Stop at that
   point: do **not** run the owner bootstrap, because the restored database
   already contains the owner.
2. Keep every VÖLUND service and timer stopped.
3. Mount or copy the authoritative CAD volume. Library roots are restored with
   their recorded absolute paths, so mount it at the same location (normally
   below `/srv/volund/library`).
4. Copy the newest `volund-*.dump` and its `.sha256` from off-host storage, then
   restore as the database owner:

   ```sh
   sudo -u volund /usr/local/sbin/volund-restore \
     /var/backups/volund/volund-20261025T031500Z.dump volund
   ```

5. Start `volundd`. Check `GET /api/v1/health` and **Administration → Betrieb**,
   then sign in with an account from the old instance.
6. Start a full scan for every library root. Derived previews and thumbnails
   are rebuilt by the preview worker; they are deliberately not part of the
   backup.

If the target already contains objects, drop and recreate the empty database
with exactly checked names; never point the script at the live `volund`
database of a running instance.
