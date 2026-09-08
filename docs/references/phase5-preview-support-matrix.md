# Phase 5 preview and browser support matrix

## Content behavior

| Content | Embedded inspection | Original fallback |
|---|---|---|
| PNG, JPEG, GIF, WebP | Safe same-origin image | Download |
| PDF | PDF.js page navigation and zoom | Download / separate same-origin open |
| TXT, Markdown, JSON, YAML and bounded text documents | Sandboxed same-origin document frame | Download / separate open |
| CSV and table-like text | Readable document view | Download |
| STEP, IGES, BREP | Native OCCT-derived GLB, thumbnail and structure manifest | Metadata plus original download |
| STL | Direct local mesh viewer; native preview may also be requested | Original download |
| 3MF, OBJ, PLY, glTF, GLB | Native derived GLB inspection | Metadata plus original download |
| Unsupported/binary/archive | Truthful metadata, size, role and path | Original download only |

Preview failure never hides the original. Every derived link is labelled as
derived, and source downloads never expose server filesystem paths.

## Browser and viewport acceptance

Release acceptance covers current stable Chromium and Firefox desktop engines
at 1440×900, 1280×800 and 1024×768, plus the responsive 390×844 layout. WebGL2,
canvas, dialog focus/cancel, keyboard tree traversal, visible focus, dark/light
backgrounds and German long-text wrapping are checked. When WebGL is unavailable
the textual structure/properties, diagnostics, metadata and download controls
remain usable.

## Performance budgets

- Manifest: at most 20,000 definitions, 20,000 occurrence nodes, depth 256,
  32 properties per object, 1,000 characters per property value.
- Problem center: 200 diagnostic runs, 50 diagnostics per run, 500 rendered
  problems, 100 items per durable bulk decision.
- File DOM: 200 files initially; explicit bounded pagination thereafter.
- Preview history: ten recent rows rendered for the opened file.
- Production-shaped visual/performance fixture: 1,500 mesh occurrences, repeated
  instances included; interaction must stay responsive and disposal must release
  renderer, geometry, materials and selection helpers.
