# ADR 0021: The primary STEP file owns CAD assembly structure

- Status: Accepted
- Date: 2026-08-29

## Context

The model page previously labeled explicit relationships between independent
catalog models as "Baugruppen & Teile". That is not the engineering structure
of a CAD assembly. VÖLUND's native STEPCAF/XCAF converter already preserves the
actual product tree, definitions, occurrences, names, and transforms in the
`assembly-manifest` artifact of each converted STEP source.

## Decision

For a model with a primary STEP source, VÖLUND treats that source and its
versioned XCAF assembly manifest as the authority for the displayed CAD
structure. The browser fetches the manifest through the authenticated artifact
endpoint and renders its nested assemblies, parts, and repeated instances.

Catalog-level model relationships are not presented as CAD components. Editing
the assembly tree itself requires changing the source in a CAD system and
importing the resulting STEP file; VÖLUND does not mutate CAD originals.

## Consequences

- The model page matches the geometry shown by the primary STEP preview.
- Existing converted assemblies need no database migration or reconversion.
- Repeated instances and nested assemblies remain visible without creating
  artificial catalog models for each node.
- Future selection or visibility controls can address stable manifest nodes and
  definitions without conflating them with catalog identity.
