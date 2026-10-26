# ADR 0034: Bulk model edit

- Status: Accepted
- Date: 2026-10-26

## Context

Curating a growing archive means repeating small edits on many models: tag a
series, move it into a collection, correct the kind or the author. The only path
was the single-model editor. That editor replaces every field, so it cannot
safely be scripted across models that other users edit at the same time.

## Decision

- `POST /api/v1/models/bulk-edit` takes 1–200 models, each with the revision
  the client saw, and one change set:
  - add or remove tags and collections, by ID;
  - set the kind;
  - set or clear the author (an empty name clears it);
  - set the license.

  Fields that are left out stay unchanged on every model.
- **All or nothing.** The models are locked in ID order, so concurrent bulk
  edits cannot deadlock. If any model is no longer at its expected revision,
  nothing changes and the 409 response names the changed models.
- **Confirmation.** Ten or more models require the confirmation
  `EDIT <count> MODELS`; the browser asks before sending it.
- **Audit.** Every changed model gets its own `model.update` audit event with
  its new revision, the changed fields, and a shared `bulkBatch` ID, so the
  model history shows bulk edits like any other edit.
- **New collection members** are appended at the end of each collection.
- **Browser.** For accounts with catalog-write access, the model list shows a
  checkbox on every card. The selection survives page changes. An action bar
  applies one change to the selection and reloads the list.

## Consequences

- Series-wide curation is one atomic, reviewable operation.
- A conflict blocks the whole batch. Users reload and reapply rather than
  getting a partially applied edit.
- Tag creation by name and the editing of names or descriptions remain
  single-model operations.
