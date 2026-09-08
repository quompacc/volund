# ADR 0010: Explicit CAD-to-glTF coordinate conversion

- Status: Accepted
- Date: 2026-08-28

## Context

OpenCascade CAD documents use the conventional +Z-up coordinate system, while
glTF and Three.js use +Y-up with -Z forward. Exporting mesh coordinates without
an explicit conversion makes mechanically upright assemblies rest on a side in
the browser's orbit controls.

## Decision

Every GLB preview export explicitly configures OpenCascade's coordinate-system
converter with Z-up input and the standard glTF output system. The conversion
is part of the native preview artifact, not a corrective rotation in the web
viewer, so every compliant glTF consumer sees the same orientation.

An asymmetric fixture verifies the exported world bounds: its CAD Z extent
becomes the glTF Y extent, and its CAD Y extent becomes the negative glTF Z
extent. Converter-version changes invalidate the immutable preview identity and
cause corrected artifacts to be generated separately from older outputs.

## Consequences

- The web viewer and future export consumers need no VÖLUND-specific rotation.
- Assembly hierarchy and source-space manifest transforms remain unchanged.
- Existing v0.10.0 GLBs are retained but superseded by v0.10.1 conversions.
