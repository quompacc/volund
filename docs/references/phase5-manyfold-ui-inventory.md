# Phase 5 interaction reference — Manyfold

Status: Read-only comparative inventory
Inspected: 2026-08-30
Reference instance: Manyfold v0.147.1

## Purpose and boundary

This inventory records useful interaction concepts observed in the user's
Manyfold instance. It is not a feature contract by itself, a visual design
template, or permission to copy implementation details. VÖLUND keeps its own
visual language, PostgreSQL authority, role model, audit rules, managed original
safety, and native Debian architecture. Phase 5 must decide each pattern against
VÖLUND's actual domain and test it independently.

The inspection was read-only. It covered dashboard/navigation, model list and
bulk edit, a large production-shaped model page, file detail and edit pages,
PDF/text/mesh previews, upload and URL import, creator and collection list/detail/
edit, activity, problem management, user preferences, instance settings and its
major subsections, integrations, and printer configuration entry points.

## Useful concepts to adapt

### Model and file workspace

- Keep file operations close to each file: a clear `Open` action, direct
  download, then less frequent actions in a compact labelled overflow menu.
- Give a file a dedicated large detail surface rather than forcing every viewer,
  metadata field, and action into the model page. The detail surface combines
  viewer, library-relative path, bounded content identity, size, download,
  slicer targets where applicable, edit, and delete.
- Group large filesets by useful relative-directory buckets and expose explicit
  progress with byte size while previews load.
- Add files to an existing model with an obvious drag-and-drop/file-picker zone
  located on that model, not only through a global import entry point.
- Use type-aware presentation: embedded document/text surfaces, interactive mesh
  canvas, image presentation, and a useful fallback for unsupported formats.
- File metadata worth evaluating includes a user-facing caption, notes,
  printed state, pre-supported state, up-axis/orientation, preview eligibility,
  relationships, and access policy. VÖLUND must only expose fields it can define,
  validate, persist, audit, and use.
- Allow an eligible file to become the model preview/thumbnail through a direct
  action, while retaining a fuller selection and regeneration workflow.

### Metadata and repeated work

- Multi-select tag controls render selected values as removable chips. Model
  lists also expose tag facets with usage counts.
- Model, author, and collection records have consistent open/edit/delete action
  placement, usage counts, sorting by name/recent/updated, and breadcrumbs.
- Contextual creation links for authors and collections appear inside the model
  editor rather than forcing the user to abandon the current task.
- Bulk model selection/edit can reduce repetitive author, collection, licence,
  tag, and library changes. Any VÖLUND bulk workflow still needs bounded size,
  per-item authorization, preview of impact, revision conflict handling, and an
  auditable partial/atomic outcome contract.
- Recent activity on the landing page gives immediate feedback that imports and
  edits took effect. VÖLUND should use its durable actor-aware history rather
  than a short-lived notification substitute.

### Problems and settings

- A central problem view makes detected issues actionable. Filters by category,
  severity, and type, per-item links, bulk selection, resolve/ignore actions,
  and honest suggested remediation are useful Phase 5 operational concepts.
- User preferences and owner/admin instance settings are separate surfaces.
  Renderer preferences can include preview auto-load limits, grid visibility and
  size, pan/zoom, background/object colors, and render style.
- Per-problem severity preferences allow `ignore`, `info`, `warning`, or
  `danger` instead of treating every diagnostic identically.
- Instance settings are grouped into named areas such as libraries, derived
  files, appearance, mesh analysis, multiuser policy, plugins, discovery,
  printers, integrations, and reporting. VÖLUND should adopt the information
  architecture while exposing only supported, runtime-owned settings.
- Statistics at the top of administration provide useful scale context, but
  must use readable labels/units and bounded, current server facts.

### Slicer handoff

- Different printable formats can expose different named slicer targets.
  Observed targets include Creality Print, Cura, ElegooSlicer, Lychee,
  OrcaSlicer, and Prusa/SuperSlicer-style handlers.
- The useful security concept is a purpose-bound, short-lived download URL
  embedded into a validated custom scheme. VÖLUND must additionally bind it to
  the authenticated resource and intended disposition, derive the externally
  reachable origin from trusted configuration, escape exactly once, avoid host
  paths, limit schemes to an allowlist, and always offer ordinary download.

## Concepts to improve rather than copy

- The inspected large model eagerly renders hundreds of file cards and preview
  loaders in one document. VÖLUND needs bounded pagination or virtualization,
  collapsible directory groups, cancellation, concurrency limits, and explicit
  load-on-demand budgets.
- Repeated icon-only menus and controls are visually compact but their meaning
  is not always self-evident. VÖLUND should use accessible names, visible labels
  for primary actions, tooltips only as supplementary help, large targets, and
  consistent keyboard/focus behavior.
- The interactive mesh canvas did not expose a useful accessible name in the
  inspected DOM. VÖLUND requires labelled viewer regions, keyboard controls,
  textual state, reset/help actions, and a non-canvas route to file metadata and
  download.
- File-type grouping based on wildcard-like headings is useful orientation but
  can be confusing and inconsistent. VÖLUND should use stable directory labels,
  counts, sort/filter state, and clear disclosure semantics.
- The activity surface states that entries older than one day are discarded.
  This is not acceptable as catalog history. VÖLUND keeps durable bounded
  history with pagination and retention rules distinct from ephemeral runtime
  notifications.
- Some labels and translations are inconsistent or overly technical. VÖLUND
  must test German wording, sentence case, units, pluralization, empty/error
  states, and long localized strings at every supported viewport.
- A preview-file selector containing hundreds of raw entries does not scale.
  VÖLUND needs searchable, type-filtered candidates, visual context, current
  selection, generated-preview candidates, and clear fallback semantics.
- Custom slicer URLs on the inspected page contained a loopback origin. VÖLUND
  must never assume `localhost`; externally reachable, trusted, configured
  origin and expiry/error behavior are required.
- The settings breadth is informative, but VÖLUND must not add inert controls.
  Every setting requires a concrete runtime owner, validation, effective value,
  origin, revision, audit behavior, and an explicit restart requirement.

## Phase 5 acceptance implications

The browser acceptance matrix must include small and very large filesets,
collapsed and expanded directory groups, progressive preview queues, load
cancellation, type filters, file detail navigation, opening/downloading every
type, context menus, tag chips, long metadata, bulk selection, problem filters,
user versus instance settings, and all keyboard/focus/assistive labels. Visual
comparison should demonstrate that VÖLUND retains its stronger layout while
meeting or exceeding the useful functional discoverability described above.
