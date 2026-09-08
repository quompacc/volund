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
| Current Firefox desktop | Automated web logic is browser-neutral, but the complete real Firefox acceptance matrix remains open |
| Narrow/mobile layout | Responsive rules exist; complete navigation, dialog, keyboard, and viewport acceptance remains open |
| JavaScript disabled | Unsupported; the browser application requires JavaScript |

## Content handling

The native converter accepts STEP, IGES, BREP, STL, 3MF, OBJ, PLY, glTF, and
GLB. Other project files may be indexed, associated, inspected through a safe
browser-native preview where implemented, or downloaded. Presence in the
catalog does not guarantee a generated 3D preview.

## Known limitations

- The full portable metadata/model export and complete independent exit package
  are not implemented.
- The full Firefox, responsive, keyboard, offline, timeout, and stale-response
  browser matrix is incomplete.
- Large-assembly performance around the 1,500-mesh target and repeated viewer
  resource release have not completed their final acceptance measurements.
- Worker restart, running-job cancellation, orphaned leases, converter timeout,
  and recovery remain under the E.2 audit gate.
- The complete slicer handoff safety and unavailable-target behavior remain
  under G.4.
- Clean installation, candidate upgrade/rollback, systemd boot, and external
  proxy acceptance remain under J.1–J.4. The tagged 0.38.3 deployment history
  does not close those gates for current `Unreleased` changes.
- VÖLUND is designed for a single instance and a small trusted team. Public
  multi-tenant SaaS operation is outside the current security model.
- No independent security review has been completed.
- Projektlizenz: AGPL-3.0-only; siehe [LICENSE](LICENSE) und
  [Drittanbieterhinweise](THIRD_PARTY_NOTICES.md). Der bereinigte Quellstand ist
  öffentlich; Release-Tag und Produktionsfreigabe bleiben getrennte Schritte.

Historical internal audit and deployment records are intentionally not part of
the public source snapshot. Dated validation results must not be read as the
status of current `main`.
