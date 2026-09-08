# ADR 0002: Native OpenCascade runs in an isolated C++ worker

- Status: Accepted
- Date: 2026-08-28

## Context

STEP import is the highest-risk component. Browser-side OCCT/WASM worked in
Stralar Studio for tessellating printable solids, but flattened the result and did
not preserve the full product structure needed by a CAD archive. Large assemblies
also need server-controlled memory, timeouts, retries, and persistent results.

Rust OpenCascade bindings remain an additional compatibility layer and do not yet
offer the confidence required for complete STEPCAF/XCAF coverage.

## Decision

The converter is a small native C++ executable linked directly to OpenCascade.
It uses STEPCAFControl/XCAF for product structure and RWGltf for preview output.
The Rust daemon invokes it as a child process rather than loading OCCT through FFI.

The worker contract is versioned. A conversion produces a GLB, an assembly
manifest, and diagnostics in a caller-provided output directory.

## Consequences

- An OCCT crash cannot terminate the API or indexer.
- systemd/cgroups can enforce resource limits.
- The converter can be tested independently with a CAD fixture corpus.
- OCCT upgrades do not require changing Rust bindings.
- Process startup overhead is accepted because conversions are durable jobs.

