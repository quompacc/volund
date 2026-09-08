# Drittanbieterhinweise

Die Projektlizenz AGPL-3.0-only gilt für VÖLUND-eigene Bestandteile.
Abhängigkeiten, übernommene Lizenztexte und enthaltene Fremdanteile behalten
ihre ursprünglichen Lizenzen und Copyrightvermerke.

## Rust

Die 236 externen Pakete des aktuellen Cargo.lock sind im
[Rust-Inventar](docs/licensing/rust-inventory.md) aufgeführt. Die
[gesammelten Originaltexte](docs/licensing/rust-notices.txt) erhalten auch
zusätzliche Copyright- und NOTICE-Dateien. Identische Texte sind dedupliziert.
Die Wahlalternativen in den Paketmanifesten werden nicht in kumulative
Pflichten umgedeutet; bei `AND` sind dagegen beide Bestandteile zu beachten.
Für `wasite 0.1.0` wird die angebotene
[Boost Software License 1.0](docs/licensing/BSL-1.0.txt) gewählt.

## Browser und Buildwerkzeuge

- Three.js 0.185.1: MIT, Copyrightvermerk der Three.js-Autoren erhalten.
- PDF.js / pdfjs-dist 6.2.108: Apache-2.0, Mozilla-Copyrightvermerk erhalten;
  zusätzliche BSD-/MIT-/OFL-Hinweise für mitgelieferte Zusatzassets ebenfalls
  aufgenommen. Diese Assets stehen weiterhin unter ihren eigenen Lizenzen.
- Vite 8.2.2: MIT-Kernlizenz für vom Build erzeugte Browserhelfer erhalten.

Die [Browserhinweise](apps/volund-web/public/THIRD_PARTY_NOTICES.txt) und
der [AGPL-Text](apps/volund-web/public/LICENSE.txt) werden von Vite unverändert
in den statischen Build kopiert. Beide Dateien bei Installation mitnehmen.
Das [npm-Inventar](docs/licensing/npm-inventory.md) erfasst zusätzlich alle
Build-/Test- und optionalen Plattformpakete. Node, Testwerkzeuge und deren
nativen optionalen Module sind keine Bestandteile der nativen Installation.

## Nativer Konverter

**VÖLUND verwendet Open CASCADE Technology.** Der Konverter beruht auf
Funktionen dieser Bibliothek. Die hier geprüfte Debian-Version 7.8.1 verwendet
LGPL-2.1 mit der Open-CASCADE-Ausnahme 1.0. Die Ausnahme erlaubt insbesondere
Objektcode aus Bibliotheksheadern unter eigenen Bedingungen, wenn dieser
Hinweis erhalten bleibt; sie hebt die übrigen LGPL-Pflichten nicht auf.
Siehe [Copyright und Ausnahme](docs/licensing/opencascade-debian-copyright.txt)
sowie [LGPL-2.1](docs/licensing/LGPL-2.1.txt).

Assimp 5.4.3 verwendet hauptsächlich BSD-3-Clause; seine Fremdanteile sind im
[Debian-Copyrightnachweis](docs/licensing/assimp-debian-copyright.txt) enthalten.
VÖLUND übernimmt weder die Windows-/Beispielmodelle noch die Testquellen dieser
Systempakete in seine eigenen Quellen.

OpenCascade, Assimp und transitive Systembibliotheken werden nach dem
Debian-Runbook über Debian bereitgestellt. Die dort installierten Copyright-
und Lizenzdateien unter `/usr/share/doc` und `/usr/share/common-licenses`
bleiben erhalten. Wer Bibliotheken selbst als Releaseartefakte bündelt, muss
zusätzlich deren genaue Versionen, Quellen, Hinweise und Lizenzpflichten
prüfen. Dieses Repository enthält keine Freigabe für ein solches Binärpaket.

## Weitergabe

LICENSE, diese Hinweise und docs/licensing gehören zu einer Quellweitergabe.
Bei Binärweitergabe müssen die einschlägigen Hinweise und der entsprechende
Quellcode ebenfalls erreichbar sein; ein Link auf irgendeinen neueren
Entwicklungsstand genügt nicht. Das weitere Verfahren und die Grenzen stehen
in [docs/licensing/README.md](docs/licensing/README.md).
