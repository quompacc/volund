# ADR 0020: Persist model maintenance and presentation orientation

Status: Accepted

## Context

Imported projects, assemblies, and parts must remain maintainable independently
of their physical source paths. CAD coordinate systems also do not always match
the orientation a user expects in the browser, even after deterministic
conversion into glTF's coordinate system.

## Decision

VÖLUND atomically replaces editable model metadata through a model-specific
PATCH endpoint. Authors are resolved or created by normalized name; tags and
collection memberships are replaced inside the same database transaction.

Each model stores three bounded presentation rotations in degrees. These values
rotate only the viewer object and never modify originals or derived geometry.
The default remains zero on every axis, preserving existing converted output.

Logical containment uses explicit directed `model_components` relationships.
The API rejects self-links and graph cycles. Removing a relationship never
deletes either model or any source file.

## Consequences

- Corrected names, types, authors, tags, collections, and orientation survive
  reloads and upgrades.
- Projects and assemblies can expose reusable child assemblies and parts
  without copying files or coupling hierarchy to directories.
- Coordinate-system conversion remains deterministic and immutable; user-facing
  presentation preferences stay separate from engineering data.
- Rich assembly-node hierarchy and per-instance transforms remain a later
  inspection concern and are not inferred from this catalog relationship.
