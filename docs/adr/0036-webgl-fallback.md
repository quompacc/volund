# ADR 0036: WebGL fallback

- Status: Accepted
- Date: 2026-10-26

## Context

The model page created a `CadViewer` for every STL file and converted preview.
Without WebGL (disabled by policy, blocked GPU, old remote desktop) the renderer
constructor threw, and the page showed only an error. With software rendering
(SwiftShader, llvmpipe, Microsoft Basic Render) the viewer worked but large
previews were unusable. The large-assembly acceptance measured a
1.5-million-triangle preview at under two frames per second and 2.3 s per
selection ([large-assembly-viewer.md](../benchmarks/large-assembly-viewer.md)).

## Decision

- The browser probes its rendering capability once per page with a throwaway
  canvas:
  - **hardware** if a WebGL context with `failIfMajorPerformanceCaveat` exists
    and its renderer string does not name a known software rasterizer;
  - **software** if only a lenient context exists, or the renderer is a known
    software one;
  - **none** if no context exists at all.
- **none:** no viewer is created. The stage explains that 3D previews are
  unavailable in this browser, while structure, file metadata, and the
  original download stay usable. File tiles show "KEIN WEBGL".
- **software:**
  - Previews up to 8 MiB load as usual.
  - Larger ones show a notice with "Vorschau trotzdem laden"; the choice is
    remembered per file for the page session.
  - Only file tiles up to 2 MiB get a live preview.
- **hardware:** behavior is unchanged.
- The large-assembly harness reports the detected capability. In real
  browsers it classified Edge and Firefox on a GPU as hardware and Edge with
  SwiftShader as software.

## Consequences

- A browser without WebGL can still be used for everything except 3D.
- Software-rendering clients no longer freeze on opening a large model; users
  stay in control of when they pay that cost.
- A coarser automatic preview profile for software clients, and server-side
  rendered images as a replacement, remain possible follow-ups.
