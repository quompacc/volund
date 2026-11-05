# ADR 0040: Automatic generated thumbnails

- Status: Accepted
- Date: 2026-11-05
- Amends: [ADR 0023](0023-explicit-thumbnail-state.md)

## Context

ADR 0023 made a model's card image explicit catalog state. The catalog already
derived a `generated` thumbnail from a ready native conversion of a linked
source, but the web interface deliberately hid it and showed a neutral
placeholder. In practice almost no model ever had such a conversion: native
conversions only ran when somebody opened a server-side preview, and STL files,
most of a typical 3D-printing library, open in the browser's local mesh viewer
without any conversion. The converter's wireframe projection was also hard to
read for dense meshes. Operators therefore saw placeholders unless they picked
an image for every model by hand.

## Decision

1. The native converter renders `thumbnail.png` with a deterministic CPU
   rasterizer: orthographic three-quarter view, z-buffered two-sided flat
   shading, silhouette and crease outlines, 2×2 supersampling, and a
   zlib-compressed RGBA PNG with a transparent background, so the image sits
   on the light and dark theme alike; the model colour lies between the two
   themes' accent colours (since 0.41.1). The PNG is typically 10–20 KB
   instead of 590 KB. It needs no GPU, display server, or additional runtime
   package beyond zlib.
2. The scheduler queues, per tick, native conversions for active models whose
   `thumbnail_kind` is `default`, using the configured default preview profile.
   It picks one renderable source per model (primary file, then master CAD,
   CAD, printable mesh), keeps at most two conversions queued or running in
   total, skips a model while one of its files converts or once a file has a
   thumbnail from the current converter version, and never retries a source
   whose conversion with the current version failed.
3. The web interface shows the `generated` image on model cards and the model
   page, labelled "Automatisch erzeugt". The selection list's empty choice now
   reads "Kein eigenes Bild (automatische Ansicht)".

The ADR 0023 state model is unchanged: a chosen source image or derived
artifact always takes precedence, and the backfill never writes model rows.

## Consequences

- New and existing models get a readable image without manual work; the first
  scheduler ticks after an upgrade work through the backlog two at a time.
- Each converter version renders once more for every placeholder model, so a
  release regenerates outdated images. Large STEP assemblies can take minutes;
  the throttle keeps interactive previews responsive.
- `zlib1g-dev` is an explicit build dependency for both deployments.
- Failed renderings stay visible in the job and problem views and are not
  retried automatically; an explicit regeneration still works.
