# Debian LXC baseline

VÖLUND is developed and deployed natively. The first validated target is Debian
13 (`trixie`) on x86-64 with OpenCascade 7.8 and PostgreSQL 17.

## Build and runtime packages

Die folgenden Schritte gelten für einen neuen, dedizierten Debian-Host mit
bereitgestelltem `/srv/volund`-Datenträger. Vorab `findmnt -T /srv/volund` prüfen;
`pg_lsclusters` ist nach Installation von `postgresql-common` verfügbar.
Bei vorhandenen Anwendungsdaten oder PostgreSQL-Clustern
nicht als Neuinstallation fortfahren; deren Umzug ist eine eigene Migration.
Diese Anleitung löscht oder ersetzt keinen bestehenden Cluster.

Vor Installation des Serverpakets die automatische Anlage des Debian-Clusters
`main` verhindern, damit der neue Cluster von Anfang an am vorgesehenen Ort
mit Prüfsummen entsteht:

```sh
sudo apt-get update
sudo apt-get install postgresql-common
pg_lsclusters
```

Nur wenn keine vorhandenen Cluster angezeigt werden, fortfahren:

```sh
printf '\ncreate_main_cluster = false\n' |
  sudo tee -a /etc/postgresql-common/createcluster.conf >/dev/null
```

```sh
sudo apt-get install \
  ca-certificates openssl wget build-essential cmake ninja-build pkg-config git \
  rustc cargo rustfmt rust-clippy \
  postgresql \
  libocct-foundation-dev \
  libocct-modeling-data-dev \
  libocct-modeling-algorithms-dev \
  libocct-data-exchange-dev \
  libocct-ocaf-dev \
  libocct-visualization-dev \
  libassimp-dev assimp-utils \
  libtbb-dev libfontconfig1-dev
```

`libtbb-dev` and `libfontconfig1-dev` are explicit because Debian's OCCT CMake
targets refer to their unversioned linker names.
`assimp-utils` is only needed for the complete CTest format-fixture matrix; the
runtime worker links against `libassimp` directly.

## Konto, Verzeichnisse und leere Datenbank zuerst anlegen

Diese Schritte müssen vor Dateirechteprüfungen, Migrationen und Dienststart
erfolgen. Das Dienstkonto erhält weder interaktive Shell noch sudo-Rechte:

```sh
sudo adduser --system --group --home /srv/volund --no-create-home volund
sudo install -d -o root -g root -m 0755 /srv/volund
sudo install -d -o volund -g volund -m 0750 \
  /srv/volund/library /srv/volund/incoming /srv/volund/derived \
  /srv/volund/scratch /srv/volund/support
sudo install -d -o postgres -g postgres -m 0700 /srv/volund/postgres
sudo install -d -o root -g volund -m 0750 /etc/volund
sudo install -o root -g volund -m 0640 /dev/null /etc/volund/volund.env
sudo pg_createcluster 17 volund --port=5432 \
  --datadir=/srv/volund/postgres/17/volund --start -- \
  --encoding=UTF8 --data-checksums --auth-local=peer --auth-host=scram-sha-256
sudo -u postgres createuser --host=/var/run/postgresql --port=5432 \
  --no-superuser --no-createdb --no-createrole volund
sudo -u postgres createdb --host=/var/run/postgresql --port=5432 \
  --owner=volund --encoding=UTF8 --template=template0 volund
sudo -u postgres psql -X -v ON_ERROR_STOP=1 -d volund \
  -c 'SHOW data_directory' -c 'SHOW data_checksums'
sudo -u volund psql -X -v ON_ERROR_STOP=1 -d volund \
  -c 'SELECT current_user, current_database()'
```

Erwartet: Datenpfad `/srv/volund/postgres/17/volund`, Prüfsummen `on`,
Rolle und Datenbank `volund`. Der Cluster nutzt den lokalen Unix-Socket und
Peer-Authentifizierung; keine Passwort-URL in der Dienstumgebung hinterlegen.
Das gemeinsame Elternverzeichnis ist betretbar, damit auch `postgres` seinen
privaten Unterbaum erreicht; Anwendungsdaten bleiben in den 0750-Unterordnern.
`pg_createcluster` darf bei belegtem Port oder bestehendem Ziel scheitern;
keine vorhandene Instanz dafür beenden oder entfernen.

## Anwendung aus dem ausgewählten Quellstand bauen

Build the locked Rust workspace and native converter, then install both
root-owned binaries:

```sh
cargo build --locked --release -p volundd
cmake -S native/cad-convert -B build/cad-convert -G Ninja \
  -DCMAKE_BUILD_TYPE=Release
cmake --build build/cad-convert --parallel 2
ctest --test-dir build/cad-convert --output-on-failure
sudo install -o root -g root -m 0755 target/release/volundd \
  /usr/local/bin/volundd
sudo install -o root -g root -m 0755 \
  build/cad-convert/volund-cad-convert /usr/local/bin/volund-cad-convert
```

Node.js is not a production dependency. Build the locked browser application on
a build host with Node.js 20.19 or newer, then install only its static output:

```sh
npm ci --prefix apps/volund-web
npm test --prefix apps/volund-web
npm run build --prefix apps/volund-web
sudo install -d -o root -g root -m 0755 /usr/share/volund/web
sudo cp -R apps/volund-web/dist/. /usr/share/volund/web/
sudo chown -R root:root /usr/share/volund/web
sudo find /usr/share/volund/web -type d -exec chmod 0755 {} +
sudo find /usr/share/volund/web -type f -exec chmod 0644 {} +
sudo -u volund test -r /usr/share/volund/web/index.html
```

## Service identity and persistent paths

The daemon runs as a system account named `volund`. Persistent files live below
the dedicated `/srv/volund` mount; application binaries are installed below
`/usr/local/bin`.

PostgreSQL uses local peer authentication: the operating-system account
`volund` owns and connects to a UTF-8 database also named `volund`. Production
initialization must enable data checksums and verify that the cluster data
directory resides below `/srv/volund/postgres`.

Vor dem ersten Dienststart den Bootstrap-Token gemäß dem Abschnitt
„First-owner bootstrap“ vorbereiten und die Befehle unter „Database migration“
ausführen. Erst danach die Unit installieren und die Loopback-API starten:

```sh
sudo install -d -o volund -g volund -m 0750 /srv/volund/support
sudo install -o root -g root -m 0644 \
  deploy/systemd/volundd.service.example /etc/systemd/system/volundd.service
sudo systemctl daemon-reload
sudo systemctl enable --now volundd.service
wget -qO- http://127.0.0.1:8080/api/v1/health
wget -qO- http://127.0.0.1:8080/ | head
```

The support directory is a transient staging boundary, not backup or user-data
storage. The daemon's systemd sandbox grants write access only to this directory
in addition to the existing library/incoming paths. Completed support archives
are returned and removed during the request. Logging and journal access are
documented in [`docs/LOGGING.md`](../../docs/LOGGING.md).

Install and activate the durable preview worker separately:

```sh
sudo install -m 0644 deploy/systemd/volund-preview-worker.service.example \
  /etc/systemd/system/volund-preview-worker.service
sudo install -m 0644 deploy/systemd/volund-preview-worker.timer.example \
  /etc/systemd/system/volund-preview-worker.timer
sudo install -m 0644 deploy/systemd/volund-scan-worker.service.example \
  /etc/systemd/system/volund-scan-worker.service
sudo install -m 0644 deploy/systemd/volund-scan-worker.timer.example \
  /etc/systemd/system/volund-scan-worker.timer
sudo install -m 0644 deploy/systemd/volund-import-cleanup.service.example \
  /etc/systemd/system/volund-import-cleanup.service
sudo install -m 0644 deploy/systemd/volund-import-cleanup.timer.example \
  /etc/systemd/system/volund-import-cleanup.timer
sudo install -m 0644 deploy/systemd/volund-scheduler.service.example \
  /etc/systemd/system/volund-scheduler.service
sudo install -m 0644 deploy/systemd/volund-scheduler.timer.example \
  /etc/systemd/system/volund-scheduler.timer
sudo install -m 0644 deploy/systemd/volund-retention.service.example \
  /etc/systemd/system/volund-retention.service
sudo install -m 0644 deploy/systemd/volund-retention.timer.example \
  /etc/systemd/system/volund-retention.timer
sudo install -m 0644 deploy/systemd/volund-backup.service.example \
  /etc/systemd/system/volund-backup.service
sudo install -m 0644 deploy/systemd/volund-backup.timer.example \
  /etc/systemd/system/volund-backup.timer
sudo systemctl daemon-reload
sudo systemctl enable --now volund-preview-worker.timer
sudo systemctl enable --now volund-scan-worker.timer
sudo systemctl enable --now volund-import-cleanup.timer
sudo systemctl enable --now volund-scheduler.timer
sudo systemctl enable --now volund-retention.timer
```

Den Backup-Timer erst nach der separaten Einrichtung und Prüfung aus
[`docs/BACKUP.md`](../../docs/BACKUP.md) aktivieren. Seine Installation allein
konfiguriert noch kein geeignetes Backupziel.

The daemon and worker intentionally have different storage and network
allowlists. Review the [sandbox baseline](../../docs/security/systemd-sandbox.md)
and complete its dynamic workflow rehearsal whenever the installed units change.

The default listener is `127.0.0.1:8080`. Set `VOLUND_LISTEN_ADDR` in
`/etc/volund/volund.env` only when deliberate direct network exposure is
required. Prefer a TLS-enabled reverse proxy for access outside the LXC.

When the reverse proxy runs on another host, install the example nftables rule
and its systemd unit before enabling the non-loopback listener. Replace the
example proxy address with the actual proxy address, keep localhost allowed,
and make the firewall unit a requirement of `volundd`. This prevents a boot or
restart window in which port 8080 is reachable by the wider LAN. Install the
drop-in from `deploy/systemd/volundd-proxy.conf.example`, then verify that the
proxy succeeds while a normal LAN client cannot connect directly to port 8080.

## First-owner bootstrap

An empty v0.22.0-or-newer instance exposes only health, setup status, guarded owner
creation, and login. Before the first start, create a one-time token readable by
the service account and point the service environment at its absolute path:

```sh
sudo install -d -o root -g volund -m 0750 /etc/volund
openssl rand -hex 32 | sudo tee /etc/volund/bootstrap.token >/dev/null
sudo chown root:volund /etc/volund/bootstrap.token
sudo chmod 0640 /etc/volund/bootstrap.token
printf '%s\n' 'VOLUND_BOOTSTRAP_TOKEN_FILE=/etc/volund/bootstrap.token' |
  sudo tee -a /etc/volund/volund.env >/dev/null
```

Den Token vor dem allerersten Start vorbereiten; dafür ist kein vorheriger
Dienststart oder Neustart erforderlich. Anschließend migrieren und den Dienst
wie oben beschrieben starten. `/etc/volund/volund.env` bleibt `root:volund 0640`.

Open the browser application, copy the token through an operator-controlled
terminal, and create the first owner. After setup reports complete, remove the
token file and its environment line, then restart the service. The database
transaction prevents a second bootstrap owner even if the stale file remains,
but removing it limits needless secret lifetime.

When VÖLUND is served through HTTPS, set `VOLUND_SECURE_COOKIES=true`. A direct
non-loopback listener refuses to start without this setting. Keep it unset only
for the loopback HTTP service behind the same-origin reverse proxy.

## Database migration

The cluster uses checksums and keeps its data directory below
`/srv/volund/postgres`. Apply migrations explicitly as the service identity:

```sh
sudo -u volund /usr/local/bin/volundd migrate
sudo -u volund /usr/local/bin/volundd database-doctor
```

Local peer authentication supplies the database identity. No production
password or database URL is required in `/etc/volund/volund.env`.

## Upgrade and full rollback

Prepare rollback artifacts before stopping anything: retain exact copies of the
installed daemon, converter, complete web tree, reviewed unit files, and current
environment file. Record their hashes and permissions. Select the new artifacts
by reviewed commit or annotated release and verify their recorded hashes.

For an upgrade with migrations, keep the external route closed and stop every
application writer before the final dump: the daemon plus preview, scan,
import-cleanup, scheduler, retention, and backup timers/services. Confirm there
is no remaining `volundd` process and no database session with application name
`volundd`. Do not stop PostgreSQL. Create a new custom-format dump as `volund`,
validate it with `pg_restore --list`, store an adjacent SHA-256 manifest, and
record its exact path. No application writer may restart before acceptance.

Run the candidate binary against the still-frozen database:

```sh
sudo -u volund /path/to/candidate/volundd migrate
sudo -u volund /path/to/candidate/volundd database-doctor
```

If migration fails, do not install or start the candidate. SQLx migrations are
transactional, but verify the old binary's `database-doctor` before reopening
the route. Investigate the error rather than marking a failed migration as
applied or editing a committed migration.

After successful migration, install the reviewed artifacts, start only the
required units, and validate schema, version, health, static assets, login, and
the catalog while the route remains closed. For migrations 0031 and 0032, a
binary-only rollback to v0.38.3 is invalid: its doctor and daemon reject the
32-migration schema. The tested full rollback is:

1. Keep the route and every application writer stopped.
2. Restore the verified frozen dump into a newly named database owned by
   `volund`; never restore over the upgraded database.
3. Point the retained old binary explicitly at that new database and require a
   successful `database-doctor`, loopback health, static-asset and catalog
   comparison against the recorded pre-upgrade state.
4. With no active database connections, retain the upgraded database under an
   unmistakable quarantine name and rename the validated restored database to
   `volund`. If either rename fails, keep services stopped and restore the
   original name before retrying.
5. Restore the retained old binaries, web tree, units, and environment; verify
   their hashes and permissions before starting. Reopen the route only after
   health and catalog acceptance.

This full rollback is lossless only while the database remains frozen from the
dump through the decision. Once an upgraded writer is admitted, do not use this
rollback without separately preserving and reconciling its new writes.

## Verification

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

npm test --prefix apps/volund-web
npm run build --prefix apps/volund-web

cmake -S native/cad-convert -B build/cad-convert -G Ninja \
  -DCMAKE_BUILD_TYPE=Release
cmake --build build/cad-convert
ctest --test-dir build/cad-convert --output-on-failure
build/cad-convert/volund-cad-convert version
```

## Isolierte Erstinstallationsprobe

`tests/first-install.py` prüft den installierten Kandidaten mit einem neuen
PostgreSQL-17-Cluster, neuem Nicht-Superuser und leerer Datenbank. Es benötigt
zusätzlich Python 3. Der Testbenutzer muss unprivilegiert sein und ein frisch
angelegtes Verzeichnis `/var/tmp/volund-j1-<Ziffern>` besitzen. Darin vorab
`bin/volundd`, `bin/volund-cad-convert` und `web/` aus den frischen Builds
root-eigen und für den Testbenutzer unveränderbar installieren. Danach:

```sh
python3 deploy/debian/tests/first-install.py /var/tmp/volund-j1-20260907
```

Der Test reserviert ausschließlich `127.0.0.1:18094`; PostgreSQL verwendet nur
einen privaten Unix-Socket unter dem Testpfad (Socketnummer 18494, kein TCP).
Bestehende Testdaten verweigert er. Er prüft Erstmigration und Wiederholung,
Peer-Authentifizierung, Prüfsummen, dynamische Konverterbibliotheken, Health,
SPA-Direktpfad, sämtliche statische Assetbytes, Owner-Bootstrap, Login und den
leeren Katalog. Eigene Prozesse beendet er im `finally`-Block. Für einen neuen
Lauf ausschließlich den exakt aufgelösten eigenen Testpfad nach bestätigtem
Prozessende entfernen; vorhandene fremde Testverzeichnisse nicht bereinigen.

Diese Probe ersetzt weder die erstmalige Paket-/Konto-/`pg_createcluster`-
Einrichtung eines neuen Hosts noch die gesonderte systemd-/Boot-/Proxyprüfung.

`tests/upgrade-rehearsal.py` accepts an exact v0.38.3 dump copied into a fresh
`/var/tmp/volund-j2-<digits>` test root. That root must also contain root-owned,
non-writable `old/volundd`, `new/volundd`, `old-web/`, and `new-web/`. It creates
only a private PostgreSQL cluster and disposable databases, and reserves
127.0.0.1 ports 18095–18097. The test covers failed migration, successful
upgrade, refusal of an artifact-only rollback, and full dump rollback. It does
not operate systemd or any installed database.

## Erstprovisionierung in einem leeren Debian-Rootfs

`tests/provision-rootfs.sh` ergänzt die vorherige Anwendungstestprobe um die
erstmalige Paket-, Konto- und Debian-Clusteranlage. Benötigt werden auf einem
ausdrücklich freigegebenen Prüfhost Debian-Paketlisten, `apt-get`, `dpkg-deb`,
`unshare`, `mount`, `chroot`, Root-Rechte, Internetzugriff auf den Debian-Spiegel
und ausreichend freier Platz für ein zusätzliches System mit frischen Builds.
Der Host benötigt keine installierten debootstrap- oder VÖLUND-Pakete.

Vorbereiten: ein neues Verzeichnis `/var/tmp/volund-j1-provision-JJJJMMTT` mit
`source.tar` aus `git archive` des ausgewählten Kandidaten, `web.tar` mit dem
Inhalt seines gebauten `dist/` sowie den drei Prüfern `provision-rootfs.sh`,
`provision-rootfs.py` und `provision-runtime.py`. Quellstand und Transferhashes
vor Ausführung abgleichen. Der zweite Parameter benennt einen vollständigen,
vertrauenswürdigen Cargo-Abhängigkeitscache zum Lockfile; er wird nur kopiert,
nicht als beschreibbares Hostverzeichnis in den Gast eingebunden.

```sh
sudo unshare --mount --pid --fork --kill-child --mount-proc \
  sh /var/tmp/volund-j1-provision-20260908/provision-rootfs.sh \
  /var/tmp/volund-j1-provision-20260908 /path/to/cargo-cache
```

Der Prüfer verlangt einen eigenen PID-Namespace und ein noch nicht vorhandenes
`rootfs`. debootstrap wird nur in den Testbaum entpackt. Alle eigentlichen
Installationen laufen im neuen Rootfs; `policy-rc.d` unterbindet Dienststarts
durch Pakete. Nur Standard-Zeichengeräte und private Proc-/PTY-/SHM-Mounts
werden bereitgestellt, keine Host-Datenträger oder Produktionsdaten. Cluster-
und App-Prozesse laufen zusätzlich in einem eigenen Netzwerk-Namespace, mit
PostgreSQL auf dessen lokalem Port 5432 und dem Testdaemon auf 18099.

Die ersten Paketblöcke und die Konto-/Cluster-, Build-, Bootstrap- und
Migrationsbefehle werden direkt aus dieser Anleitung gelesen. Builds erfolgen
vor der Konto-/Clusterprobe; der Lesbarkeitstest als `volund` folgt unmittelbar
nach der Kontoanlage. Node läuft ausschließlich auf dem separaten Web-Buildhost.
Die Laufzeitprobe prüft Dateirechte, Peer-Authentifizierung ohne Superuser,
Konverterbibliotheken, Migration plus Wiederholung, Assetbytes, SPA, Bootstrap,
Login und Leerkatalog. Das ist keine systemd-, Boot-, Caddy- oder Firewallprobe.

Nach Prozessende zunächst bestätigen, dass keine Testmounts oder Testprozesse
mehr vorhanden sind. Erst dann ausschließlich den exakt aufgelösten eigenen
Prüfbaum samt Archiven entfernen. Ein fehlgeschlagenes Rootfs wird nicht als
frische Installation wiederverwendet; die vollständige Provisionierung beginnt
erneut leer. Vorbestehende Caches, Builds und Testverzeichnisse bleiben erhalten.

## Isolierte Lastprobe für kleine Teams

`tests/team-load.py` misst ein festes, leselastiges Profil mit acht parallelen
Sitzungen, 20 Requests pro Sekunde, 15 Sekunden Aufwärmzeit und 180 Sekunden
Messzeit. Der unprivilegierte Testbenutzer benötigt einen frischen Pfad
`/var/tmp/volund-k2-<Ziffern>` mit einem root-eigenen, nicht beschreibbaren
`bin/volundd` und einem geprüften Dump direkt unter diesem Pfad. Danach:

```sh
python3 deploy/debian/tests/team-load.py \
  /var/tmp/volund-k2-20260908 \
  /var/tmp/volund-k2-20260908/baseline.dump
```

Die Probe legt einen PostgreSQL-17-Cluster mit Prüfsummen und ausschließlich
privatem Unix-Socket an. Der Daemon bindet nur `127.0.0.1:18098`. Gemessen und
begrenzt werden Durchsatz, Fehlerquote, p95/p99-Latenz, Daemon- und
PostgreSQL-RSS, kombinierte CPU-Zeit, Datenbankwachstum, App-Verbindungen und
Lock-Warter; außerdem muss der Queue-Zustand stabil bleiben. Der `finally`-Pfad
beendet ausschließlich die eigenen Prozesse und prüft, dass der Testport wieder
bindbar ist. Das Profil misst interaktive Katalog- und Verwaltungslesezugriffe;
Worker- und Konverterdurchsatz sind Gegenstand der separaten Worker- und
Konverterprüfungen.
