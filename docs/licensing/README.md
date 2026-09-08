# Lizenzentscheidung und Weitergabe

Am 2026-09-08 hat der Betreiber AGPL-3.0 angenommen, nach der ausdrücklichen
Empfehlung `AGPL-3.0-only`. Daher gilt ausschließlich Version 3, ohne
automatische Wahl späterer Fassungen. Maßgeblich ist [LICENSE](../../LICENSE).
Der Lizenztext wurde unverändert vom GNU-Projekt übernommen.

Die Lizenz umfasst die eigenen Rust-, TypeScript-/Web- und C++-Quellen sowie
eigene Skripte und Dokumentation, sofern kein abweichender Hinweis besteht.
Fremdcode und dessen Lizenztexte bleiben unter den jeweils angegebenen
Bedingungen. CAD-Dateien, importierte Modelle und Kataloginhalte erhalten
durch Verarbeitung keine neue Softwarelizenz.

## Bewertung der Abhängigkeiten

| Bereich | Ergebnis für diesen Quellstand |
| --- | --- |
| Rust: 236 externe Pakete | Vollständiger Lockfilebestand einschließlich optionaler Plattformzweige erfasst; keine fehlende Lizenzangabe. MIT, Apache-2.0, BSD, Zlib, Unicode und angebotene Wahlalternativen lassen sich unter Beibehaltung ihrer Hinweise mit AGPLv3 verbinden. |
| `matchit` | `MIT AND BSD-3-Clause`: beide Texte sind enthalten, keine bloße Wahl. |
| `unicode-ident` | `(MIT OR Apache-2.0) AND Unicode-3.0`: Unicode-Text zusätzlich zu den angebotenen Softwarelizenzen erhalten. Auch separate Unicode-COPYRIGHT-Dateien anderer Pakete sind enthalten. |
| `crc-catalog` | Beide Texte liegen in `LICENSES/` und sind vollständig aufgenommen. |
| `wasite 0.1.0` | Das geprüfte Paketarchiv enthält keine Lizenzdatei. Sein zugehöriger Upstream-Commit bestätigt `Apache-2.0 OR BSL-1.0 OR MIT`. Für diesen optionalen WASI-Zweig wird BSL-1.0 gewählt und deren offizieller Volltext mitgeliefert. |
| npm: 120 Pakete | Alle deklarierten Lizenzen erfasst; 66 Pakete lokal vorhanden. MPL-2.0 betrifft Lightning-CSS-Buildwerkzeuge und optionale Bindings, keine AGPL-Umlizenzierung dieser Dateien. |
| Aktueller Browserbuild | Source-Maps weisen Third-Party-Module aus `three` und `pdfjs-dist` aus. Zusätzlich MIT-Hinweis für Vite-Browserhelfer mitgeliefert. Node-Canvas und plattformfremde native Module werden nicht mit der Anwendung installiert. |
| PDF.js-Zusatzassets | Separate Lizenztexte für CMaps, ICC, Fonts und WASM erhalten. Die OFL-Fonts bleiben OFL und werden nicht zu AGPL-Software umdeklariert; ihr Vorhandensein im npm-Paket bedeutet nicht ihre Verwendung im Browserbuild. |
| OpenCascade 7.8.1 | LGPL-2.1 mit Ausnahme 1.0; dynamisches Systemlinking, eigener Quellcode und prominenter OpenCascade-Hinweis vorgesehen. Bibliothekslizenz und Möglichkeit zum Austausch der Bibliothek bleiben erhalten. |
| Assimp 5.4.3 | Debian-Copyrightdatei einschließlich Fremdanteilen geprüft. CC-BY-3.0 betrifft `contrib/zip/test/minunit.h`, GPL-3+ die Debian-Paketierung; diese Quellen werden nicht in VÖLUND übernommen. |

Die npm-Inventur ist keine Behauptung, alle plattformfremden Binärpakete
heruntergeladen zu haben. Fehlende reine Buildpaket-Lizenzdateien bei
`stackback` und Plattformbindings werden durch ihre MIT-Metadaten nicht zu
VÖLUND-Artefakten. Eine Weitergabe von node_modules, Buildtool-Binaries oder
anderen zusätzlichen Paketen ist neu auf vollständige Originalhinweise zu
prüfen. Der aktuelle Installationsvertrag liefert diese nicht aus.

## Reproduzierbare Grundlage

- Rust-Metadaten: `cargo metadata --locked --offline --format-version 1`.
  Die SHA-256 jedes zugehörigen `.crate`-Archivs wurde mit Cargo.lock
  verglichen. Lizenz-, COPYING-, NOTICE- und COPYRIGHT-Dateien wurden aus
  den gzip/Tar-Archiven gelesen, einschließlich Lizenzunterverzeichnissen.
  Vorhandene entpackte Textdateien wurden bytegleich abgeglichen.
- Die 162 unterschiedlichen Rust-Texte in `rust-notices.txt` sind nach
  SHA-256 geordnet; pro Text ist ausschließlich CRLF nach LF normalisiert.
  Das [Inventar](rust-inventory.md) bindet Paketversion, Archivhash und Text-ID.
- npm: `packages` in package-lock.json, ohne den Wurzeintrag. Vorhandene
  package.json-Versionen wurden gegen das Lockfile geprüft. Browsertexte
  stammen aus den vorhandenen Three.js-/PDF.js-Paketen; Source-Maps des
  frischen Builds bestätigen die eingesetzten Paketmodule.
- Nach Lockfile- oder Buildänderungen Inventare und Texte neu abgleichen;
  ein altes Inventar ist keine Freigabe für neue Abhängigkeiten.

## Pflichten bei Weitergabe und Netzwerkbetrieb

1. Lizenz- und Copyrightvermerke erhalten, Änderungen kenntlich machen und
   die AGPL-Gewährleistungsausschlüsse nicht durch unbegründete Zusagen ersetzen.
2. Bei Weitergabe von Objektcode den dazugehörigen vollständigen Quellcode
   gemäß AGPL §6 bereitstellen, einschließlich erforderlicher Buildskripte.
   Fremdanteile und gegebenenfalls zusätzlich gebündelte Bibliotheken einbeziehen.
3. Wer eine veränderte netzwerkfähige Version betreibt, muss deren entfernten
   Nutzern nach AGPL §13 ein deutliches, kostenloses Angebot zum Bezug des
   entsprechenden Quellcodes machen. Ein bloßer Lizenzlink ersetzt das nicht.
4. Der öffentliche Quellstand und das reproduzierbare Quellarchiv werden durch
   L.4 und die GitHub-CI geprüft. Ein späterer Tag oder Binärrelease muss diese
   Bindung erneut bestätigen und darf keine zusätzlichen Artefakte ungeprüft
   aufnehmen.

Die Entscheidung ändert weder rückwirkend alte Commits noch die Freigabe
privater Betriebsnachweise. L.2, L.4 und L.5 sind bestanden.
Diese betrieblichen Freigabegrenzen sind keine zusätzlichen Lizenzbedingungen.

## Herkunft der übernommenen Texte

LICENSE und `apps/volund-web/public/LICENSE.txt` sind fremde Lizenztexte;
`rust-notices.txt`, `web-notices.txt` und die öffentliche Browserhinweisdatei
sind generierte Sammlungen fremder Lizenztexte. Die Debian-Copyrightdateien,
LGPL-2.1 und BSL-1.0 sind unverändert übernommene Fremdtexte. Sie sind damit
ausdrücklich von der 600-Zeilen-Grenze für handgeschriebene Dateien ausgenommen.
Die eigenen Markdown-Dokumente und Tests bleiben unter dieser Grenze.
Originale Schlussleerzeichen in den drei Hinweissammlungen bleiben für den
Hashabgleich erhalten; ausschließlich diese Dateien haben in .gitattributes
eine Ausnahme von der Schlussleerzeichenprüfung. Eigene Quellen nicht.

- [AGPL-3.0-Originaltext](https://www.gnu.org/licenses/agpl-3.0.txt)
- [GNU-Kompatibilität, auch AGPLv3](https://www.gnu.org/licenses/gpl-faq.en.html#AllCompatibility)
- [Mozilla: MPL und Weitergabe](https://www.mozilla.org/en-US/MPL/2.0/FAQ/)
- [wasite: Paketcommit und Lizenzwahl](https://github.com/ardaku/wasite/blob/0c72934ad329ab4670eb017581640c2d6eceb289/Cargo.toml)
- [Boost-Lizenztext](https://www.boost.org/LICENSE_1_0.txt)
- [OpenCascade: Debian-Copyright](https://metadata.ftp-master.debian.org/changelogs/main/o/opencascade/opencascade_7.8.1+dfsg1-3_copyright)
- [Assimp: Debian-Copyright](https://metadata.ftp-master.debian.org/changelogs/main/a/assimp/assimp_5.4.3+ds-2_copyright)
