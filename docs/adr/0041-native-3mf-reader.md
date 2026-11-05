# ADR 0041: Native 3MF reader

- Status: Accepted
- Date: 2026-11-05

## Context

The converter imported 3MF through Assimp. Slicer projects from Bambu Studio,
PrusaSlicer and OrcaSlicer, the most common source of 3MF files in a printing
library, use the 3MF production extension: the root model only holds objects
with `<component p:path="/3D/Objects/…model">` references, and the geometry
lives in separate parts of the package. Assimp ignores these references and
fails with "aiScene::mNumMeshes is 0", so every such file failed to convert and
never got a preview or generated thumbnail. Some packages also use ZIP64.

## Decision

`native/cad-convert/src/three_mf.cpp` reads 3MF itself: a read-only ZIP reader
(stored and deflated entries, ZIP64) built on zlib, a small tag scanner for the
model XML, and resolution of objects, components (including `p:path` parts,
percent-encoded names) and build items with their transforms. It returns the
object graph, not flattened meshes. The importer turns each object into one
shared XCAF definition and rigid placements into located components, so
repeated parts stay single definitions in the assembly manifest; scaled or
mirrored placements, which XCAF locations cannot express, are baked into the
mesh. Empty placeholder objects are skipped. Assimp stays the fallback when
the native reader rejects a package.

Bounds: 1 GiB per package and per part, 30 million triangles, 32 nesting
levels, cycle detection.

## Consequences

- Slicer 3MF files convert and get previews and thumbnails; all 101 3MF files
  of the reference homelab library convert.
- Colours and materials from 3MF are not read yet.
- The reader has its own unit test (`volund-three-mf-test`).
