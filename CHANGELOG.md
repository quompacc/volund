# Changelog

All notable changes to VÖLUND are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Version headings are labels. Forge-specific comparison links remain omitted
until a signed release tag is created.

## [Unreleased]

### Added

- Bibliotheks-Onboarding nach ADR 0035: Ein „Probelauf“ beim Anlegen liest
  den Ordner nur lesend und meldet Dateien, Größe, erkannte CAD-Formate,
  übersprungene Symlinks, die größten Dateien, Blocker, an denen ein Scan
  scheitern würde (unlesbare Verzeichnisse, Namen ohne gültiges UTF-8), und
  Überschneidungen mit registrierten Bibliotheken
  (`POST /api/v1/libraries/dry-run`). Laufende Scans zeigen in der
  Bibliotheksverwaltung ihren Fortschritt und lassen sich mit Bestätigung
  abbrechen; fehlgeschlagene Scans zeigen ihre Ursache.
- Abnahme der Viewer-Leistung für große Baugruppen
  (`apps/volund-web/tests/viewer-performance.mjs`): echte Browser (Edge über
  CDP, Firefox über WebDriver BiDi) messen Laden, Freigabe, Bildzeiten beim
  Drehen, Auswahl, Isolieren und Strukturbaum für 1.500 Meshes bis
  1,5 Millionen Dreiecke gegen feste Budgets. Ergebnisse in
  `docs/benchmarks/large-assembly-viewer.md`; ein optionaler
  Software-Rendering-Lauf zeigt die Grenze ohne GPU.
- Massenbearbeitung nach ADR 0034: In der Modellliste lassen sich Modelle
  seitenübergreifend auswählen und gemeinsam bearbeiten (Tag hinzufügen oder
  entfernen, Sammlung hinzufügen oder entfernen, Art, Autor).
  `POST /api/v1/models/bulk-edit` wendet die Änderung auf alle Modelle oder
  auf keines an, prüft jede Revision und nennt bei Konflikten die betroffenen
  Modelle; ab zehn Modellen ist eine Bestätigung nötig. Jedes Modell erhält
  einen eigenen Verlaufseintrag mit gemeinsamer Batch-ID.
- Zentrale Problemansicht nach ADR 0033: Die neue Ansicht „Probleme“ (mit
  Zähler in der Navigation) und `GET /api/v1/problems` bündeln
  Konvertierungs- und Baugruppenprobleme aller Modelle, fehlende oder nicht
  verfügbare Originale und fehlgeschlagene Bibliotheks-Scans, gefiltert nach
  Schweregrad und Status. Jeder Eintrag führt zum betroffenen Modell oder
  zur Bibliotheksverwaltung; Ignorieren und Erledigen bleiben auf der
  Problemseite des Modells.
- Serverseitige Modellliste nach ADR 0032: `GET /api/v1/models/search` mit
  Volltextsuche (Name, Beschreibung, Autor, Tags), Tag- und Artfilter,
  Sortierung (zuletzt bearbeitet, Name, neueste) und Seiten samt Summen. Die
  Modellübersicht nutzt sie mit Suchfeld, Filtern und Seitenwechsel; der
  Filterzustand steht in der URL und lässt sich als Lesezeichen teilen. Das
  Dashboard und der Navigationszähler laden nicht mehr den ganzen Katalog.
- Gespeicherte Ansichten der Modellliste: Jedes Konto kann bis zu 50 benannte
  Filterkombinationen speichern, auswählen und löschen (Migration 0034,
  `/api/v1/saved-views`).
- Boot-, Proxy-, Upgrade- und Rückwegprobe für J.2–J.4
  (`deploy/debian/tests/boot-probe.py`): bootet ein frisches Debian-13-Rootfs
  mit echtem systemd per `systemd-nspawn`, installiert nach dem Runbook, prüft
  Caddy-TLS-Proxy, nftables-Sperre und Firewall-Reihenfolge, eine echte
  OCCT-Vorschau, Neustart, Upgrade, `systemctl stop` mitten in einer
  Konvertierung und Rollback.
- `deploy/backup/volund-restore` spielt einen geprüften Backup-Dump in eine
  leere Ersatzdatenbank ein: Prüfsumme und Archiv werden vorab verifiziert,
  belegte Zieldatenbanken abgewiesen, der Restore läuft in einer Transaktion.
  Ein Linux-Integrationstest probt Sicherung und Restore mit echtem
  `pg_dump`/`pg_restore` und verlangt einen identischen, betriebsbereiten
  Katalog. BACKUP.md enthält das Runbook für eine Ersatzinstanz.
- Portabler Export nach ADR 0029: „Exportieren“ auf jeder Modellseite liefert
  ein ZIP mit allen verfügbaren Originalen unter `originals/<root>/<pfad>`,
  `SHA256SUMS` und einem versionierten `manifest.json`
  (`volund-model-export-v1`). Originale werden beim Packen gegen ihre
  Katalog-Prüfsumme verifiziert; fehlende Dateien werden im Manifest als
  ausgeschlossen markiert. Owner und Administratoren exportieren unter
  Administration → Betrieb die gesamten Katalog-Metadaten als konsistentes,
  versioniertes JSON (`volund-metadata-v1`) ohne Geheimnisse und Serverpfade.
  Beide Formate haben JSON-Schemas in `contracts/`; Beschreibung in
  `docs/EXPORT.md`.
- Umschaltbare Darstellung (System, Hell, Dunkel) in der Kopfleiste. Die
  Auswahl wird pro Browser gespeichert; „System“ folgt der Betriebssystem-
  Einstellung, und der 3D-Viewer mit Hintergrund „System“ folgt dem gewählten
  Seitenthema.
- Eigenes Vorschaubild hochladen: Im Vorschaubild-Panel eines Modells lädt
  „Neues Bild hochladen“ ein PNG-, JPEG-, GIF- oder WebP-Bild über den
  regulären Erweiterungsimport (Eingangsspeicher, SHA-256, Prüfung, atomare
  Übernahme) hoch und setzt es im selben Commit als Vorschaubild. Beim Import
  eines neuen Modells kann neben der Vorschaubild-Auswahl ein Bild außerhalb des
  Pakets gewählt werden; es wird nach der Übernahme ergänzt. Würde das Bild eine
  vorhandene Bibliotheksdatei verschieben oder kollidieren, bricht der Vorgang
  ab und verwirft seinen Entwurf.
- `VOLUND_API_PROXY` setzt das API-Ziel des Vite-Dev-Servers, etwa für einen
  Tunnel zu einer Testinstanz. Mit `VOLUND_API_SECURE_COOKIES=true` schreibt
  der Proxy nur die eigene `http`-Herkunft des Dev-Servers auf `https` um, damit
  Instanzen mit Secure-Cookies die Herkunftsprüfung akzeptieren; fremde
  Herkünfte bleiben abgelehnt.

### Fixed

- Slicer-Übergabe nach ADR 0031: Jeder Download prüft erneut Konto, Modell,
  Druckbar-Markierung, Datei und Ziel; ein Token bedient höchstens acht
  Anfragen und wird nie gecacht (`no-store`, `no-referrer`). Pro Konto sind
  höchstens zehn offene Übergaben möglich (429). Unvollständig konfigurierte
  Ziele (z. B. ohne `VOLUND_PUBLIC_BASE_URL`) liefern 503
  `slicer_handoff_misconfigured`, und die Oberfläche erklärt das, statt
  wirkungslose Knöpfe anzubieten. Reagiert kein lokaler Slicer, erscheint nach
  2,5 s ein Hinweis mit Download-Link. Migration 0033 ergänzt den
  Nutzungszähler; abgelaufene Übergaben entfernt die Retention.
- Worker-Leases nach ADR 0030: Vorschau- und Scan-Worker halten jeden
  übernommenen Job über eine Sitzungssperre der Datenbank. Stirbt ein Worker
  (OOM, Neustart, Kill), erkennt der nächste Worker-Lauf den verwaisten Job
  sofort statt nach zwei Stunden; die Warteschlange bleibt bei Parallelität 1
  nicht mehr stehen. Verlorene Konvertierungen werden „fehlgeschlagen“
  (manueller Retry), verlorene Scans neu eingereiht und nach drei verlorenen
  Versuchen als fehlgeschlagen beendet. Späte Abschlüsse eines bereits
  übernommenen Laufs veröffentlichen nichts mehr, und ein Fehler in einem
  Batch-Task bricht parallele Jobs nicht mehr ab.
- Vorschau- und Scan-Worker beenden sich bei SIGTERM/SIGINT geordnet: Der
  Konverter-Prozessbaum wird beendet bzw. der unbestätigte Scan verworfen und
  der Job wieder eingereiht (höchstens dreimal). Die systemd-Beispielunits
  setzen `KillMode=mixed`, `TimeoutStopSec=30s` und für den Vorschau-Worker
  `OOMPolicy=continue`, damit ein OOM-beendeter Konverter nicht die Unit stoppt.
- Konvertierungs-Timeouts zählen erst ab Erhalt der Konvertierungssperre; bei
  `jobs.conversionConcurrency` > 1 scheitern wartende Jobs nicht mehr fälschlich
  am Timeout. Die 30-Sekunden-Gnadenfrist von SIGTERM bis SIGKILL greift jetzt
  tatsächlich, und Zeitüberschreitungen erhalten den Status „timed-out“ statt
  „failed“ (Exit-Status 124 bei `volundd convert`).
- Eigene Konvertierungsprofile mit Mesh-Deflection scheiterten immer, weil
  `volundd convert` `--linear-deflection`/`--angular-deflection` ablehnte; die
  Werte werden jetzt geprüft und an den Konverter weitergereicht.
- Die tägliche Retention entfernt verwaiste Konvertierungsverzeichnisse unter
  `derived/` (`.work/`, `failed/` und unveröffentlichte `jobs/`), sobald kein
  Artefakt sie referenziert und sie alt genug sind; das Ergebnis meldet
  `orphanDirectories` und `orphanBytes`.
- RustSec meldet für `zip 4.6.1` drei Lücken in `ZipArchive::extract` und
  `ZipStreamReader::extract`. VÖLUND ruft beide nicht auf; der ZIP-Import liest
  Einträge einzeln und weist Symlinks und Sonderdateien ab. Die Ausnahme ist im
  Advisory-Register begründet und befristet, `tests/dependency_guard.rs` verhindert
  eine künftige Nutzung. `zip 9` setzt Rust 1.88 voraus, Debian 13 liefert 1.85.
- CI-Schritt „Test and build web application“ war seit dem 2026-10-25 rot,
  weil `npm audit` neue Sicherheitsmeldungen für Buildwerkzeuge meldete. Vite
  8.3.5, Vitest 4.1.12 und happy-dom 21.0.4 schließen sie; Lizenzinventar und
  Vite-Hinweis sind nachgezogen. Der ausgelieferte Browsercode war nicht
  betroffen.
- Die Tag-Vorschläge im Importformular klappen nicht mehr von selbst auf, sobald
  die Tag-Liste geladen ist, sondern erst, wenn das Tag-Feld fokussiert ist.

### Changed

- Sachlicheres Oberflächendesign: neutrale Grautöne mit einem Petrol-Akzent,
  Systemschrift statt Serifen, Strich-Icons statt Runen- und Formglyphen sowie
  keine Verläufe, Raster- oder Leuchteffekte mehr. Alle Farben kommen aus
  zentralen Design-Tokens (`theme.css`); Kontraste beider Themes werden
  automatisch gegen 4,5:1 geprüft.
- Stylesheets nach Bereichen aufgeteilt (`imports.css`, `raw-catalog.css`), damit
  jede Datei unter der 600-Zeilen-Grenze bleibt.
- Nüchternere Beschriftungen, etwa „CAD-Bibliothek“ statt „The Sovereign CAD
  Vault“ und „Übersicht“ statt „Die Schmiede“.
- CI prüft mit `scripts/check-line-limit.sh` die 600-Zeilen-Grenze für
  handgeschriebene Codedateien; als `@generated` markierte Dateien sind
  ausgenommen.
- ADR „Model maintenance and presentation orientation“ von der doppelt
  vergebenen Nummer 0020 auf 0028 umnummeriert; die Entscheidung bleibt gleich.
- SUPPORT.md und die Unter-READMEs beschreiben den aktuellen Stand: J.1
  abgeschlossen, bestandene Responsive-Matrizen, gestreamter Import-Upload und
  die PostgreSQL-Vorschauwarteschlange.

### Removed

- Ungenutzter Demo-Viewer und Mock-Modelldaten. Das Katalogmodul heißt jetzt
  `catalog-views.ts` (`mountCatalogView`) statt `mockup.ts`, weil es den echten
  Katalog rendert.

## [0.39.0] - 2026-09-08

### Added

- Bereinigter, quellcode-only Snapshot ohne bisherige Git-Historie im
  öffentlichen GitHub-Repository `quompacc/volund`, mit real gerenderten
  Produktoberflächen in der README, GitHub-CI und reproduzierbarem Quellarchiv.
  Interne Betriebsnachweise, Binärpakete, `node_modules` und Systembibliotheken
  bleiben ausgeschlossen.

- Betreiberentscheidung für AGPL-3.0-only umgesetzt: offizieller Lizenztext,
  Rust-/npm-Metadaten, Drittanbieterhinweise und Inventare für 236 externe
  Rust- sowie 120 npm-Pakete. Lizenztexte werden im Webbuild mitgeliefert;
  drei Vertragsprüfungen sichern Inhalt und Zuordnung. L.1, die bereinigte
  öffentliche Historie und deren Quellartefakt-Gates sind abgeschlossen.

- Read-only-Bestandsprüfung der sieben verbleibenden Auditpunkte mit
  belegten Freigabeabhängigkeiten und Lizenzinventar als Vorarbeit zu L.1.
  Kein weiterer Hauptpunkt abgeschlossen; konkrete Betreiberlizenz fehlt.

- Reproduzierbarer Firefox-WebDriver-Abschluss für 119 Responsive-Fälle,
  elf Ansichten bei echtem 200-Prozent-Browserzoom, Touch-/Mausbedienung,
  Verlauf, direkte URL und native Dialoge; aktuelle Chromium-Gegenmatrix.

- Bestandene Erstprovisionierung in einem leeren Debian-13-Rootfs mit originalen
  Runbook-Paket-/Konto-/Clusterbefehlen, frischen nativen Builds, Migrations-
  wiederholung, Peer-Authentifizierung, Assetprüfung, Bootstrap und Login.
  Reproduzierbarer Prüfer mit getrennten Mount-, PID- und Netzwerk-Namespaces;
  J.1 abgeschlossen, systemd-/Boot-/Proxyabnahme bleibt separat.

- Reproduzierbare Chromium-Responsive-Matrix mit elf Ansichten, sechs
  Adminbereichen, sieben Viewports und ausschließlich synthetischen Daten.

- Reproducible small-team load measurement against an isolated PostgreSQL
  restore with fixed throughput, latency, memory, CPU, database-growth,
  connection-pool, lock-wait and queue-stability limits.

- Isolated upgrade and full-rollback rehearsal from the installed v0.38.3
  database shape to migrations 31/32, including an atomic migration failure,
  catalog fingerprints, old-schema refusal and lossless frozen-dump recovery.
  The Debian runbook now keeps writers stopped through acceptance and explicitly
  requires database restoration when rolling back to v0.38.3.

- Reproduzierbare isolierte Debian-Erstinstallationsprüfung mit frischen Builds,
  leerem PostgreSQL-Cluster, Peer-Authentifizierung, Migrationswiederholung,
  unveränderbaren installierten Assets, Owner-Bootstrap und Login. Die Anleitung
  ergänzt Konto-, Pfad- und Clusteranlage sowie die Reihenfolge vor Dienststart.
  Die neue Rootfs-Erstprovisionierung schließt die damalige Paket-/Kontolücke.

- Real-browser import lifecycle acceptance covering mixed ZIP/file intake,
  reload and persistent resume, rename, exact cancellation, expiry cleanup and
  capacity rejection with matching database state.

- Real-browser import acceptance covering multiple loose files and a ZIP,
  complete metadata, a persistent file-path conflict, two atomic commits,
  catalog verification and matching original-file checksums.

- Persistent Linux/PostgreSQL model-validation coverage for invalid IDs,
  Unicode and length boundaries, empty values, metadata-name and slug
  collisions, and foreign tag, collection and primary-file references.

- Real-browser administration recovery evidence covering an interrupted
  loopback API connection, a visible error, double-click-safe retry, restored
  content and preservation of the selected administration area.

- End-to-end viewer acceptance evidence connecting a freshly generated native
  STEP/GLB/assembly-manifest set to the real API, model page and file dialog,
  including tree, selection, visibility, camera and integrity checks.

- A browser-visible 1,500-mesh viewer lifecycle benchmark with fixed load-time
  and GPU-resource-release limits across repeated load and clear cycles.

- A complete inventory of all six administration areas and 31 action families,
  mapped to successful and negative browser, API and persistence evidence.

- Native worker recovery regressions covering real converter descendants,
  running scan cancellation, process restart, stale claims, timeout/failure,
  and idempotent successful retry.

- Persistent Linux storage-failure regressions for write-protected and
  unavailable libraries, plus a bounded private-tmpfs ENOSPC test with cleanup
  and successful upload retry.

- Persistent Linux/PostgreSQL path-boundary regressions for root redirection,
  cross-library and outside destinations, and concurrent moves of one source,
  verifying audit continuity, original bytes and stable identity after rescanning.

- Persistent authentication-boundary coverage for secure-cookie configuration,
  CSRF/origin enforcement, login throttling and browser session transitions.

- Persistent Slicer-handoff regressions for target allowlisting, strict HTTPS
  origins, unavailable-target and non-printable errors, safe UI escaping and the
  original-download fallback when no target application is configured.

- Public installation, security-reporting, support-matrix, maturity, and known-
  limitation documentation without deployment-specific host dependencies.

- Exhaustive runtime authorization coverage for every protected read route,
  automatic HEAD handling, all restricted write-role combinations and all
  anonymous protected writes.

- Persistent administration UI coverage for a confirmed user-role change,
  including its exact in-app confirmation contract.

- Persistent catalog-query coverage for 120-row lists, all four sort modes,
  stable page boundaries, empty results, combined filters, Unicode and URL
  special characters.

- Persistent queue ordering tests for request age and ID ties, invitation-token
  negative cases, and real daemon stderr/stdout, response and support-bundle
  secrecy checks under an injected database failure.

- Persistent PostgreSQL/API coverage for session expiry, absolute lifetime caps,
  role/status revocation, reactivation, private session inventories and owner/admin
  self-lockout protection. Owner race tests now exclude unrelated fixture locks
  from their transaction barriers.

- Persistent streaming assertions for original and derived artifact MIME types,
  byte-range capability and their private versus immutable cache contracts.

- Scanner regression coverage for simultaneous new and returning files,
  preserving source identity and an existing primary model relationship.

- Native converter regressions for concrete sRGB assembly colors, STEP data
  without renderable faces, and clear rejection of unsupported file formats.

- Persistent Linux/PostgreSQL regression coverage proving that library rename,
  deactivation, stale writes and retries preserve root identity, catalog links,
  thumbnail source references and original file checksums.

- Controlled concurrent library metadata API tests: preserve independent PATCH
  fields and characterize the open missing-revision conflict finding B.3-F1.

- Concurrent library registration and audit/commit failure coverage for library
  creation and edits, including unchanged state and successful retries.

- Concurrent edit/delete and edit/retirement API coverage for policy objects,
  checking final state against successful audit events.
- Regression coverage for preserving queued scans and conversion snapshots
  when retiring profiles or deleting schedules, including repeated requests.
- Deterministic concurrent-update API tests for profiles and scan schedules,
  verifying a single winning revision and successful audit event.
- API regression coverage for profile/schedule validation, built-in profile
  protection, stale profile revisions and confirmed schedule deletion.
- Persistent API coverage for repeated password resets, forced password changes,
  owner protection and credential secrecy in inventories and stored audit logs.
- Regression tests for concurrent invitation acceptance, login-lock recovery,
  public authentication origin boundaries and session cookie attributes.
- Persistent API regression coverage for disallowed write-role combinations,
  action-specific lifecycle permissions, foreign import drafts and actor-owned
  lifecycle plans, including intentional administrator lifecycle exceptions.

### Fixed

- Sichtbare Navigationstasten für schmale Ansichten sowie Umbruchkorrekturen
  für Kopfaktionen, lange Modellnamen, Bibliothekspfade, Rohdateikatalog und
  Importübersicht. Browserregressionen bestätigen zuvor überlaufende Inhalte.
- Changelog-Historie unverändert in verlinkte Archive unterhalb der Grenze von
  600 Zeilen ausgelagert.

- Redact direct private-network, operator-account and workstation-key-path
  identifiers from the current documentation and firewall example. Historical
  publication remains blocked until the repository history and private
  operational records have an explicitly approved disposition.

- Isolate asynchronous view results from later navigation and end stalled JSON
  reads and mutations with honest timeout guidance instead of an endless
  loading or submission state.

- Restore focus to the originating import-draft action after cancelling its
  dialog, and raise desktop navigation labels, counters and account guidance
  above the normal-text contrast threshold.

- Replace import-draft rename and cancellation prompts with accessible in-page
  dialogs, and translate ordinary and ZIP intake-capacity failures into
  actionable German guidance.

- Replace the import conflict resolver's browser prompt with an accessible
  in-page target-path dialog so conflicts remain resolvable in constrained
  Chromium environments.

- Bound directly created model slugs to 160 characters and reject PDF, image,
  archive or unknown files as primary model sources instead of assigning them
  a CAD role.

- Make every job and quarantined original reachable from Administration with
  API-backed pagination, preserve filters and the active area between pages,
  and recover automatically when concurrent changes empty the current page.

- Prevent repeated account and user-management submissions while an action or
  confirmation is pending; preferences and instance settings also reject
  duplicate submissions and remain retryable after a failed request.

- Discard and release stale GLB or STL results that finish after viewer disposal
  or a newer model load, including geometries, materials and bound textures.

- Stop preview polling immediately after a model view is disposed instead of
  issuing one additional request after the pending poll delay.

- Propagate assembly visibility changes through nested viewer hierarchies,
  restore ancestors when showing one child, and retain a selected part when an
  unrelated sibling is hidden.

- Report legacy assembly manifests with missing or unsupported transform
  conventions or color spaces in the model problem center, matching the viewer's
  rejection instead of showing a misleading clean model status.

- Keep assembly-tree visibility buttons and selected details synchronized with
  the 3D viewer when isolating previously hidden parts and restoring all parts.
  Add three interaction regressions and a synthetic browser fixture.

- Cancelling a managed preview now terminates the runner's entire process
  group, including converter children. A bounded supervisor also stops hung
  descendants and lock waiters instead of leaving work running after cancellation.

- Uploads now check the final asynchronous write with `flush` before syncing
  and publishing. A full filesystem can no longer report a successful upload
  with the expected byte count while publishing an empty file.

- Slicer public-base configuration now rejects paths, queries, fragments and
  user information instead of accepting values that are not HTTPS origins.

- Catalog folder pages beyond the final row now retain the correct filtered
  total instead of reporting zero.

- Running conversions no longer report a fabricated 50/100 progress value;
  unavailable measurements are explicit in the job API and administration UI.

- Internal API failures now emit sanitized structured diagnostics instead of
  writing raw database errors, potentially including credentials, to stderr.

- Present invalid library paths, duplicate registrations and stale library
  revisions as actionable German messages in the German administration UI.

- Ignore duplicate library actions while validation, confirmation or a mutation
  is pending; release the UI guard after success, failure or cancellation.

- Require a current library revision for metadata and activation changes,
  including the revision captured before UI confirmation. Concurrent edits
  return a conflict instead of silently overwriting another change (B.3-F1).

- Serialize manual scan admission with library deactivation; a request that
  loses the activation lock cannot enqueue or promote a scan to full mode.
- Exclude test-isolation advisory locks when synchronizing library registration
  and policy terminal-race regression tests.

- Require directory search permission as well as read permission when validating
  library paths and reporting storage readability; read-only searchable libraries
  remain supported.

- Commit manually queued library scans and their audit records atomically;
  failed audit writes or commits no longer leave queued work or promote an
  existing incremental scan to a full scan after an error response.

- Bind profile retirement and schedule deletion to the revision captured before
  confirmation; reject stale or missing revisions without changing the object.

- Allow disabling a scan schedule after its library has been disabled, while
  still requiring an active library to enable the schedule.
- Allow retrying failed administration loads without duplicate retry requests;
  ignore late load failures after the administration view has been disposed.
- Return authenticated clients to the login flow on API HTTP 401 responses,
  including uploads and support downloads, without reloading on HTTP 403.
- Show an accurate empty-catalog message on the dashboard and model list when
  no tag filter is selected, distinguishing indexed files from imported models.
- Apply browser-origin validation to public authentication mutations before
  login, invitation acceptance or first-owner setup can change state.
- Serialize account updates before taking target-user locks, preventing deadlocks
  between concurrent owner demotions while preserving the last active owner.

- Enforce library-administration permissions when listing quarantined files.
- Preserve temporary login locks during account metadata changes; unlock only
  when an active status is explicitly requested.
- Recheck invitation expiry after acquiring row locks, refusing activation when
  the token expires while its acceptance request waits.

- Reject unexpanded archives during import review and commit, including legacy
  plans that would otherwise publish a ZIP rejected by archive validation.
- Keep ordinary upload cleanup alive after HTTP request cancellation; release
  partial-upload reservations promptly and finish completed streams consistently.

- Serialize upload completion before item updates and progress aggregation, so
  concurrent file uploads preserve byte counts and the completed draft status.
- Resume ZIP imports without requiring server-extracted files; transfer only
  outstanding ordinary files while retaining ZIP expansion retries and strict
  path/size validation of the browser selection.
- Recheck ZIP draft lifecycle and wall-clock expiry after acquiring publication
  locks, refusing expired or terminal drafts before publishing extracted items.

- Keep mixed ZIP/file imports in uploading state while any item is outstanding;
  mark them uploaded only when all items are complete, preserving resume routing.

- Clean attempt-owned ZIP extraction files when requests are cancelled, retaining
  cleanup ownership in running blocking workers until their writes have finished.

- Isolate temporary ZIP extraction files per attempt and recheck publication
  under the draft lock so overlapping retries cannot delete each other's files
  or publish duplicate entries.

- Use shared, serialized capacity accounting for ZIP publication and previews;
  reject archive expansion when pending staging cleanup consumes the remaining
  capacity, and allow retry after cleanup without duplicate extracted entries.

- Serialize concurrent import capacity checks and draft reservations in one
  database transaction, preventing parallel previews from overbooking storage.

- Count uncleaned terminal import uploads against incoming capacity until cleanup
  finishes; retain reservations while imports are committing.

- Retry staging cleanup for committed imports, report cleanup failures without
  rolling back successful imports, and retain their pending bytes in storage
  accounting until cleanup is acknowledged.

- Make every import review item reachable through incremental 200-item pages,
  including conflict resolution controls beyond the initial page.
- Reject skipping selected primary or thumbnail import items under draft locks;
  flag legacy invalid skip plans as conflicts and hide primary skip controls.

- Use the configured primary import item for review highlighting and base-folder
  selection instead of the original automatic candidate, matching commit behavior.

- Preserve explicit import conflict resolutions across review refreshes, including
  renamed targets, skipped items and reuse decisions; recheck alternate target
  occupancy before allowing publication.

- Restore the selected primary flag on existing model-source links during import
  updates, including reused and relocated files, without clearing an unchanged
  primary when no replacement is selected.

- Plan only one relocation per matched source in a multi-file import; preserve
  additional filenames as new copies and reject stale duplicate-relocation plans.
- Finish previous import cleanup before staging new operations, preventing a
  later item from retiring an uncommitted original path.
- Reject internal quarantine paths during import resolution and publication,
  including unsafe plans saved by older versions.

- Preserve the original path on interrupted managed moves; commit a durable
  cleanup intent with the new catalog path and move audit before unlinking.
- Reject the internal quarantine directory and its descendants as managed move
  destinations, preventing files from disappearing from subsequent scans.

- Preserve catalog source paths when an import is cancelled before commit;
  guard staged files and persist post-commit relocation cleanup for retry.
- Reject symbolic-link directories in managed move destinations so rescanning
  retains the original source-file identity.

- Verify streamed file contents against their catalog hash before ETag, range or
  full responses, rejecting same-length changes until the catalog is refreshed.
- Keep import paths relative when an identical primary CAD file exists directly
  in the library root.
- Reject expired import drafts after acquiring commit locks, before publishing
  any files or catalog changes.

- Preserve the catalog-owned file through source lifecycle audit/commit failures;
  persist and retry post-commit cleanup for quarantine, recovery and purge.
- Recheck credentials under consistent user/credential locks before session
  creation and own-password replacement, rejecting stale in-flight passwords.

- Apply the retained-preview count per content object instead of globally, so
  unrelated sources do not evict each other's recent previews.
- Allow role and display-name edits on invited accounts without changing their
  invitation status.
- Accept valid invitations even after an administrator has provisioned a
  temporary password, replacing that credential with the invitee's password.
- Show an explicit unknown-state error when the quarantine inventory fails to
  load instead of claiming that no files are quarantined.

- Require in-app exact confirmation for session revocation, account status,
  account role and execution-policy changes, restoring the selected role after
  cancellation.
- Keep the protected Owner role visible to administrators without offering
  Owner escalation for other accounts.
- Use in-app confirmation and inline errors for profile, schedule and retention
  actions; prevent duplicate policy submissions while an action is pending.
- Label scan counters as newly hashed and discovered files, avoiding misleading
  incomplete progress for successful incremental scans with unchanged files.

- Use accessible in-app exact-confirmation dialogs for library and job actions
  instead of unsupported native browser prompts.
- Restore scan buttons after cancellation or request failure and retain the
  queue confirmation after refreshing the administration data.

- Keep shared administration feedback visible outside inactive tabs, including
  invitation links and access errors.
- Retain the password form reference across asynchronous submission so a
  successful password change resets the form instead of reporting a UI error.

## Frühere Versionen

- [0.38.3 bis 0.21.0](docs/changelog/0.38.3-bis-0.21.0.md)
- [0.20.0 bis 0.1.0](docs/changelog/0.20.0-bis-0.1.0.md)
