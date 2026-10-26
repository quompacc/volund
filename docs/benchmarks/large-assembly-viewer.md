# Large-assembly viewer acceptance

- Date: 2026-10-26
- Harness: `apps/volund-web/tests/viewer-performance.mjs` runs
  `tests/viewer-performance-browser.html`. That page uses the production
  `CadViewer` and assembly structure tree with synthetic GLB fixtures; there is
  no API access and no production data.
- Host: Intel Core i9-14900, 64 GiB RAM, Windows 11 Pro, NVIDIA GeForce RTX 4070
  SUPER, 60 Hz frame pacing.
- Browsers: Microsoft Edge 156 (headless, CDP) and Firefox 158 (headless,
  WebDriver BiDi). An extra Edge run used the SwiftShader software renderer as
  an informative lower bound for clients without a usable GPU.

## Budgets

Each run loads the assembly once, then repeats load and clear 10 times, orbits
the camera for 180 frames, selects three parts, isolates and resets, toggles
visibility, and mounts the 1,501-node structure tree before selecting a node in
it.

| Check | Budget |
| --- | --- |
| First and every repeated load | ≤ 10,000 ms |
| Model meshes after load | exactly 1,500 |
| After each clear | 0 model meshes; GPU geometry and texture counters back at baseline |
| Orbit frame interval, p95 | ≤ 50 ms |
| Select one part | ≤ 50 ms |
| Isolate, reset | ≤ 150 ms |
| Toggle visibility | ≤ 50 ms |
| Structure tree mount, node selection | ≤ 750 ms, ≤ 100 ms |
| JS heap growth, cycles 3–10 after forced GC (Chromium only) | ≤ 8 MiB |

Every interaction time includes two animation frames (about 33 ms at 60 Hz).
That is how long the result takes to become visible, not only the time the
call itself spends.

## Fixtures

| Variant | Meshes | Geometries | Triangles | GLB |
| --- | ---: | ---: | ---: | ---: |
| repeated-instances | 1,500 | 1 shared | 18,000 | 0.1 MB |
| unique-geometry | 1,500 | 1,500 | 84,000 | 6.3 MB |
| unique-geometry-1.5m-triangles | 1,500 | 1,500 | 1,500,000 | 82.8 MB |

The last variant matches the adaptive web preview of the VORON 2.4 benchmark
assembly ([voron-2.4-assembly.md](voron-2.4-assembly.md)): 1,689 instances,
1.46 million triangles, 48 MB GLB.

## Results

| Browser | Variant | First / worst load | Frame p95 | Select | Isolate | Tree mount | Result |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| Edge | repeated-instances | 63 / 35 ms | 18.3 ms | 34.6 ms | 32.2 ms | 80 ms | passed |
| Edge | unique-geometry | 85 / 63 ms | 18.1 ms | 34.6 ms | 30.6 ms | 65 ms | passed |
| Edge | 1.5 M triangles | 241 / 202 ms | 18.3 ms | 35.5 ms | 30.4 ms | 66 ms | passed |
| Firefox | repeated-instances | 107 / 78 ms | 18.0 ms | 34.0 ms | 31.0 ms | 49 ms | passed |
| Firefox | unique-geometry | 208 / 228 ms | 18.0 ms | 34.0 ms | 31.0 ms | 36 ms | passed |
| Firefox | 1.5 M triangles | 534 / 464 ms | 18.0 ms | 34.0 ms | 31.0 ms | 35 ms | passed |
| Edge, SwiftShader | repeated-instances | 37 / 42 ms | 29.2 ms | 52.2 ms | 45.4 ms | 82 ms | informative: select |
| Edge, SwiftShader | unique-geometry | 72 / 63 ms | 37.5 ms | 141.8 ms | 52.5 ms | 115 ms | informative: select |
| Edge, SwiftShader | 1.5 M triangles | 189 / 503 ms | 612.9 ms | 2,307.6 ms | 1,194.4 ms | 1,180 ms | informative: not usable |

**Resource release.** In every run, GPU geometry and texture counters
returned to the baseline after each clear, and no model mesh remained. In
Edge, the JS heap after forced garbage collection grew by 0.32–0.37 MB in
total over cycles 3–10 (about 45 KB per cycle), far below the budget and
independent of the model size. The 78 MB growth of a first, uncollected
measurement was garbage, not a leak. Firefox does not expose heap size to
pages, so that check is skipped there.

## Conclusions

- On a desktop GPU, the 1,500-mesh target and an assembly the size of the
  benchmark are interactive in both browsers: rendering keeps up with the
  display, and load, clear, and selection are far inside their budgets.
- Repeated load and clear does not leak renderer resources or JS memory.
- Without a usable GPU, light previews stay usable, but a 1.5-million-triangle
  preview renders below two frames per second. That case belongs to the WebGL
  fallback work: a coarser automatic profile, or a structure-only view, when
  rendering is software-only.
- Real STEP assemblies differ from these fixtures in their materials, colors,
  and hierarchy depth. Their conversion side is measured separately in
  `voron-2.4-assembly.md`.

## Reproducing

On a machine with Edge or Chrome and/or Firefox:

```sh
cd apps/volund-web
npm ci
node tests/viewer-performance.mjs --report viewer-performance.json
VOLUND_PERF_SOFTWARE=1 node tests/viewer-performance.mjs   # add the software lower bound
```

`EDGE_BINARY`, `CHROME_BINARY`, and `FIREFOX_BINARY` select the browsers;
missing ones are skipped. The harness exits with status 1 if a gating run fails.
