# Export and exit formats

VÖLUND keeps originals in ordinary library folders and offers two exports so that
models and metadata stay usable without VÖLUND. The design is recorded in
[ADR 0029](adr/0029-portable-model-and-metadata-export.md).

| Export | Who | How | Format |
| --- | --- | --- | --- |
| Model package | every signed-in role | **Exportieren** on a model page, or `GET /api/v1/models/{id}/export` | zip, `volund-model-export-v1` |
| Catalog metadata | owner, administrator | **Administration → Betrieb → Katalog-Metadaten exportieren**, or `POST /api/v1/exports/metadata` | JSON, `volund-metadata-v1` |

Derived previews, conversions, and thumbnails rendered from CAD are not exported.
They can be rebuilt from the originals at any time.

## Model package

```text
<slug>-volund-export.zip
├── manifest.json                         model, components, files (contracts/model-export-v1.schema.json)
├── SHA256SUMS                            one line per included original
└── originals/<rootKey>/<relativePath>    the unchanged original files
```

Verify the package without VÖLUND:

```sh
unzip frame-volund-export.zip -d frame
cd frame && sha256sum -c SHA256SUMS
```

The `files` array of `manifest.json` lists every file linked to the model.
Included files have `archivePath`. Files missing on disk or in quarantine have
`included: false` and an `exclusionReason` (`missing` or `not-available`). If an
original no longer matches its catalog checksum, the export is refused with HTTP
409 and `model_export_source_changed`. Rescan the library, then try again.

The archive is staged in `/srv/volund/support` and removed before the download
starts. That volume needs free space for the largest model you export, and at
most two exports run at the same time.

## Catalog metadata

The JSON document (`contracts/metadata-export-v1.schema.json`) is one consistent
snapshot. It contains:

- `libraryRoots` and `sourceFiles`: root key, relative path, SHA-256, size,
  format, lifecycle state, and missing flag. Together with the library volume,
  these locate every original.
- `fileDependencies` and `fileMoves`;
- `authors`, `tags`, and `collections` (with ordered `modelIds`) and `models`
  (license, author, tags, components, thumbnail selection, and per-file
  metadata such as role, caption, notes, printable or printed, and orientation);
- `users`: e-mail, display name, role, and status. Passwords and sessions are
  never exported.
- `catalogHistory`: the same model, file, author, tag, and collection events the
  history views show.

All references use public UUIDs. `omitted` names everything intentionally left
out. A complete exit therefore consists of the metadata JSON plus a copy of the
library volume.

## Relation to backup

The daily database backup ([BACKUP.md](BACKUP.md)) remains the recovery path for
a VÖLUND instance. The exports are the exit path, readable without VÖLUND and
without PostgreSQL. Re-importing a metadata export into a new instance is not
implemented yet.
