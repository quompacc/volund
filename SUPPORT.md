# VÖLUND support matrix

VÖLUND 0.39.0 is a pre-1.0 alpha technical foundation. The matrix records
environments that have concrete repository evidence; it is not a promise of
general platform compatibility or a declaration of full production readiness.

## Runtime and build matrix

| Area | Status | Current boundary |
| --- | --- | --- |
| Debian 13 x86-64 | Validated target | Native systemd services, PostgreSQL 17, OpenCascade 7.8 and Assimp |
| Other Linux distributions/architectures | Unqualified | May build, but no release acceptance evidence exists |
| Windows and macOS runtime | Unsupported | Windows is used for repository work and web checks; the native product target remains Debian |
| PostgreSQL 17 | Validated target | Local peer-authenticated deployment and isolated test databases |
| Node.js 20.19+ | Build-time only | Required for the locked web build, not for the installed runtime |
| Containers | CI only | Permitted for disposable CI jobs; not a supported application runtime |
| Reverse proxy | Operator supplied | Must preserve same-origin behavior, TLS, secure cookies, upload limits, and backend isolation |

## Browser matrix

| Browser/context | Status |
| --- | --- |
| Current Chromium desktop | Primary tested browser; several authenticated flows have real-browser evidence |
| Current Firefox desktop | Reproducible WebDriver responsive matrix (119 cases, 200 % zoom, touch/mouse, history, native dialogs) passes; keyboard, offline, timeout, and stale-response acceptance remains open |
| Narrow/mobile layout | Firefox and Chromium responsive matrices (seven viewports, synthetic data) pass; keyboard and real-device acceptance remains open |
| JavaScript disabled | Unsupported; the browser application requires JavaScript |

## Content handling

The native converter accepts STEP, IGES, BREP, STL, 3MF, OBJ, PLY, glTF, and
GLB. Other project files may be indexed, associated, inspected through a safe
browser-native preview where implemented, or downloaded. Presence in the
catalog does not guarantee a generated 3D preview.

## Known limitations

- Restoring a backup into an empty replacement database is scripted
  (`volund-restore`) and rehearsed in CI; a complete clean-host restore of a
  production instance has not been accepted yet.
- Model packages and the versioned catalog metadata export are available
  (docs/EXPORT.md). A single whole-instance package including originals and the
  re-import of a metadata export into a new instance are not implemented.
- The keyboard, offline, timeout, and stale-response browser acceptance is
  incomplete; the automated Firefox and Chromium responsive matrices pass.
- Large-assembly viewer acceptance passed on 2026-10-26 in Edge and Firefox on a
  desktop GPU: 1,500 meshes up to 1.5 million triangles load in well under a
  second, orbit at display rate, and release every renderer resource across ten
  cycles (docs/benchmarks/large-assembly-viewer.md). Without a GPU (software
  rendering), previews of that size are not interactive; a WebGL fallback is
  still open.
- Worker leases, immediate recovery of lost jobs, orderly SIGTERM stop,
  lock-aware conversion timeouts, and orphaned-directory cleanup are
  implemented and covered by Linux integration tests with real worker
  processes (ADR 0030), and a systemd stop mid-conversion is part of the
  boot probe below.
- Slicer handoff safety (G.4, ADR 0031): bounded, re-validated download
  tokens, a per-account limit, an explicit unavailable state, and a browser
  hint when no local slicer reacts. Real desktop slicers (PrusaSlicer,
  OrcaSlicer, Bambu Studio) have not been exercised against it in an automated
  test.
- The J.2–J.4 boot probe (`deploy/debian/tests/boot-probe.py`) passed on
  2026-10-25 for v0.39.0 → current `main` → v0.39.0 in a freshly bootstrapped,
  booted Debian 13 container: runbook installation, systemd boot and reboot,
  Caddy TLS proxy with the nftables upstream restriction, a real OCCT preview,
  upgrade, worker stop mid-conversion, and rollback. It runs in a container on
  a development host, not on a production LXC; the operator's production
  upgrade remains a separate step. Clean installation (J.1) passed for 0.39.0
  in an empty Debian 13 rootfs.
- VÖLUND is designed for a single instance and a small trusted team. Public
  multi-tenant SaaS operation is outside the current security model.
- No independent security review has been completed.
- Projektlizenz: AGPL-3.0-only; siehe [LICENSE](LICENSE) und
  [Drittanbieterhinweise](THIRD_PARTY_NOTICES.md). Der bereinigte Quellstand ist
  öffentlich; Release-Tag und Produktionsfreigabe bleiben getrennte Schritte.

Historical internal audit and deployment records are intentionally not part of
the public source snapshot. Dated validation results must not be read as the
status of current `main`.
