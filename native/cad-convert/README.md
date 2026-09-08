# Native CAD converter

`volund-cad-convert` is VÖLUND's isolated native CAD worker. It accepts STEP,
IGES, BREP, STL, 3MF, OBJ, PLY, glTF, and GLB and writes a browser preview plus
a loss-aware assembly manifest.

```sh
volund-cad-convert convert \
  --input assembly.step \
  --output /srv/volund/derived/job-id
```

The format is selected from the extension (`.stp`/`.igs` aliases are accepted).
`--format step|iges|brep|stl|3mf|obj|ply|gltf|glb` overrides that selection for
misnamed files. STEP, IGES, and BREP retain exact B-Rep geometry internally;
the other formats enter through their native triangle meshes.

`--profile web` is the default and derives linear deflection from the model
bounding-box diagonal (`diagonal / 1000`) with an angular deflection of `0.8`
radians. `--profile fine` uses `diagonal / 5000` and `0.5` radians. Explicit
`--linear-deflection` and `--angular-deflection` values override either profile.
The resolved values are recorded in `result.json` for reproducibility.

## Output contract

- `preview.glb`: binary glTF generated from the XCAF document
- `thumbnail.png`: deterministic bounded raster generated from the meshed scene
- `assembly.json`: definitions, instances, names, parent-local transforms, and
  sRGB colors plus bounded allowlisted definition properties
- `diagnostics.json`: machine-readable success or failure diagnostics
- `result.json`: contract-v1 summary with source SHA-256, byte size, artifact
  names, triangle count, definition count, and instance count

XCAF label entries such as `0:1:1:2` are conversion-scoped identifiers. They
must not become durable database identities: another OCCT release or a changed
source file may assign different entries. VÖLUND's durable identity layer will
combine source content hashes with explicit product/instance paths.

## Format semantics

STEP/XCAF provides the richest product hierarchy. OBJ and glTF/GLB use OCCT's
native XCAF readers. STL and PLY are inherently flat; they become one or more
mesh definitions. 3MF is imported through Assimp into XCAF and retains shared
mesh definitions, component nodes, build transforms, and supported material
colors. Assimp 5.4 may expose numeric object IDs instead of optional 3MF object
names; the original file remains authoritative and untouched.
IGES and BREP retain geometry but may not carry a complete modern assembly
model because their source formats do not consistently define one.

The worker does not yet expose PMI, texture files, layers or external-reference
provenance in `assembly.json`. Definition properties are deliberately limited to
the bounded values supplied by the supported native readers.
Resource limits and cancellation belong to the Rust parent process and systemd,
not to this worker.

## Test corpus

`volund-cad-fixture` creates deterministic STEP, IGES, and BREP fixtures. CTest
also derives STL, OBJ, PLY, glTF, and GLB files and packages a native 3MF
component fixture. The matrix verifies every import, GLB output, format marker,
non-empty mesh, 3MF instance transform, explicit format override, and invalid
STEP diagnostics.
