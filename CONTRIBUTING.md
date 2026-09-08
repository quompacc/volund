# Contributing to VÖLUND

## Code structure

Handwritten source files must not exceed 600 physical lines. This is a ceiling,
not a target. Split code when responsibilities diverge, even when the file is
still shorter. Generated and vendored files are exempt when clearly marked.

Modules should describe one domain responsibility. Functions should do one
coherent job, use explicit inputs and results, and keep filesystem, database,
process, and serialization boundaries visible.

## Tests and quality gates

Every new function needs automated coverage. Private helpers may be exercised
through the public behavior they implement; non-trivial algorithms deserve
focused unit tests. Every bug fix must add a regression test that fails without
the fix. Documentation-only changes do not require a new test.

Before a milestone commit, run all relevant checks:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cmake --build build/cad-convert
ctest --test-dir build/cad-convert --output-on-failure
```

The same gates run in [Gitea Actions](docs/CI.md) on disposable Debian and
PostgreSQL containers. A green Windows run does not replace the Linux gate.

Do not commit knowingly broken intermediate states to `main`. Commit and push
after each coherent milestone has passed its applicable checks. Use short,
imperative commit subjects that describe the delivered outcome.

## Versioning and changelog

VÖLUND follows Semantic Versioning while it is pre-1.0:

- patch: compatible fixes and internal improvements;
- minor: new capabilities or intentional contract changes during 0.x;
- major: stable-contract breaking changes after 1.0.

The Rust workspace version and native worker version represent one product and
must stay aligned. Wire-contract versions and database migration numbers evolve
independently because persisted data needs explicit compatibility handling.

For a release:

1. Move completed entries from `Unreleased` in `CHANGELOG.md` into a dated
   version section.
2. Update the Rust workspace and native CMake versions together.
3. Run the complete quality gates on Windows and the Debian target where
   applicable.
4. Commit, push, and create an annotated `vMAJOR.MINOR.PATCH` tag.

## Lizenz von Beiträgen

Eigene Beiträge zu VÖLUND werden unter AGPL-3.0-only eingebracht, soweit keine
ausdrücklich dokumentierte, kompatible Fremdlizenz gilt. Neue Abhängigkeiten
erfordern einen Abgleich mit [den Lizenzhinweisen](docs/licensing/README.md)
und eine Aktualisierung des zugehörigen Inventars. Fremde Copyright- und
Lizenzhinweise erhalten; keine fremden Rechte durch die Projektlizenz ersetzen.

## Persistence and repository safety

Once a database migration has been committed and shared, it is immutable. Any
correction is a new forward migration with its own tests. CAD originals are
read-only to scanners and converters. Only the explicit managed-move workflow
may relocate them inside a library, with stable identity, no-overwrite behavior,
rollback, and auditing. Never commit credentials, large CAD fixtures, generated
artifacts, database files, or backups.

Consequential architecture decisions belong in `docs/adr/`. Development and
production deployment remain native on Debian without Docker. The only Docker
exception is the dedicated, isolated Gitea CI runner LXC, where containers may
be used for ephemeral builds and tests. CI containers must never become a
VÖLUND runtime dependency or receive production CAD data, credentials, or
database access.
