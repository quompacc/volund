# VÖLUND mit Docker Compose

Zweite unterstützte Bereitstellung neben der nativen Installation aus
[`deploy/debian/README.md`](../debian/README.md); Begründung und Abbildung der
systemd-Sandbox in [ADR 0039](../../docs/adr/0039-container-deployment.md).
Die Images entstehen aus denselben Quellen und Debian-13-Paketen wie der
native Build. Ein veröffentlichtes Image gibt es nicht; gebaut wird lokal oder
im CI-Runner.

## Aufbau

| Container | entspricht | Netz | Schreibzugriff |
|---|---|---|---|
| `volund-db` | PostgreSQL-17-Cluster | keins (nur Socket) | Datenbank |
| `volund-migrate` | `volundd migrate` + `database-doctor` | keins | – |
| `volund` | `volundd.service` | Proxy-Netz, kein Host-Port | library, incoming, support |
| `volund-preview` | Vorschau-Worker (10 s) | keins | derived, scratch |
| `volund-scan` | Scan-Worker (5 s) | keins | – |
| `volund-scheduler` | Scheduler (30 s) | keins | – |
| `volund-cleanup` | Import-Aufräumen (stündlich) | keins | incoming |
| `volund-retention` | Aufbewahrung (täglich) | keins | derived |
| `volund-backup` | `volund-backup` (täglich) | keins | Backupziel |

Alle App-Container laufen als `volund` (uid/gid 10010) mit schreibgeschütztem
Root-Dateisystem, ohne Capabilities und mit `no-new-privileges`. PostgreSQL
lauscht nicht auf TCP. Alle Prozesse erreichen es über das gemeinsame
Socket-Verzeichnis per Peer-Authentifizierung, also ohne Datenbankpasswort.
Die Worker starten erst, wenn `volund-migrate` erfolgreich war.

## Images bauen

Im Repository-Wurzelverzeichnis des ausgewählten, geprüften Stands:

```sh
docker build -f deploy/docker/Dockerfile --target app -t volund:0.42.0 .
docker build -f deploy/docker/Dockerfile --target db  -t volund-db:0.42.0 .
```

Der App-Build führt die Web-Tests und die CTests des Konverters aus. Der
Rust-/PostgreSQL-Gesamttest bleibt Aufgabe des CI (`docs/CI.md`).

## Erstinstallation

```sh
install -d -m 0755 /srv/volund
install -d -o 10010 -g 10010 -m 0750 \
  /srv/volund/library /srv/volund/incoming /srv/volund/derived \
  /srv/volund/scratch /srv/volund/support /var/backups/volund
install -d -o 999 -g 999 -m 0700 /srv/volund/postgres
install -d -m 0700 /srv/volund/secrets
openssl rand -hex 32 > /srv/volund/secrets/postgres-superuser
chown 999:999 /srv/volund/secrets/postgres-superuser
chmod 0400 /srv/volund/secrets/postgres-superuser
cp deploy/docker/compose.yaml /srv/volund/
cp deploy/docker/.env.example /srv/volund/.env   # Pfade und URL anpassen
```

Das Superuser-Passwort verlangt das PostgreSQL-Image. Über TCP ist es nie
erreichbar, weil der Cluster nicht auf TCP lauscht.

Die Bibliothek (`VOLUND_LIBRARY`) und das Backupziel (`VOLUND_BACKUP_DIR`)
müssen auf verschiedenen Geräten liegen, sonst verweigert `volund-backup` den
Lauf. Das Proxy-Netz (`VOLUND_PROXY_NETWORK`) muss existieren. Der
Reverse-Proxy erreicht den Dienst dort als `volund:8080`, mit TLS und
`VOLUND_PUBLIC_BASE_URL` als öffentlicher Adresse.

Vor dem ersten Start den Bootstrap-Token wie in der nativen Anleitung anlegen
und als `VOLUND_BOOTSTRAP_TOKEN_FILE` an den Dienst `volund` übergeben, etwa per
Override-Datei mit zusätzlichem schreibgeschütztem Mount. Danach:

```sh
docker compose up -d
docker compose logs volund-migrate
```

Die optionale Stralar-Anbindung unter `/stralar/`
([ADR 0042](../../docs/adr/0042-stralar-proxy.md)) ist im Container-Betrieb noch
nicht verfügbar: der Container erreicht keinen Loopback-Dienst des Hosts. Sie
folgt mit einem eigenen Stralar-Container; bis dahin `VOLUND_STRALAR_URL` nicht
setzen.

## Umzug einer nativen Installation

Wie beim nativen Upgrade alle Schreiber der alten Installation stoppen und einen
frischen Custom-Dump erstellen. Bibliothek, `derived` und `incoming` mit
Eigentümer 10010 kopieren, weil die Bibliothekspfade in der Datenbank als
`/srv/volund/library/…` gespeichert sind. Dann nur die Datenbank starten und den
Dump als `volund` einspielen:

```sh
docker compose up -d volund-db
docker compose cp volund.pgdump volund-db:/tmp/volund.pgdump
docker compose exec -u volund volund-db \
  pg_restore --dbname volund --no-owner --role volund --exit-on-error /tmp/volund.pgdump
docker compose exec volund-db rm /tmp/volund.pgdump
docker compose up -d
```

`volund-migrate` meldet danach ein bereites Schema, oder es wendet fehlende
Migrationen an, wenn das Image neuer ist als der Dump.

## Upgrade und Rückweg

Es gelten die Regeln aus [`INSTALLATION.md`](../../INSTALLATION.md):

1. Das bisherige Image-Tag als Rückweg behalten.
2. `docker compose stop` für alle App-Container, die Datenbank läuft weiter.
3. Einen geprüften Dump mit `volund-backup` erstellen:
   `docker compose run --rm volund-backup /usr/local/sbin/volund-backup`.
4. Neues Tag in `.env` eintragen, dann `docker compose up -d`.

Ein reiner Image-Rückweg ist nur gültig, wenn der neue Stand keine Migration
angewendet hat. Sonst gilt der vollständige Dump-Rückweg aus
`deploy/debian/README.md`.

## Tests

`deploy/docker/tests/volund-loop-test.sh` prüft den Timer-Ersatz. Geprüft werden:
- Wiederholung, auch nach einem fehlgeschlagenen Lauf;
- die Weitergabe von `SIGTERM` an einen laufenden Job;
- sofortiges Ende während der Pause;
- Ablehnung ungültiger Aufrufe.

Der Test läuft im CI und lokal mit `sh deploy/docker/tests/volund-loop-test.sh`.
