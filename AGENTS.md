# VÖLUND repository rules

- Keep handwritten code files at or below 600 physical lines. Split code by
  responsibility before reaching the limit; generated and vendored files are
  exempt and must be clearly identified.
- Keep classes, modules, and functions focused. File boundaries should follow
  domain responsibilities rather than arbitrary size-only splitting.
- Every new function must be covered by an automated test. Test internal helper
  behavior through its public interface unless a focused unit test is clearer.
  Every bug fix requires a regression test.
- Run all relevant formatting, linting, unit, integration, and contract checks
  before committing. Commit and push after each coherent, successful milestone.
- Follow Semantic Versioning. Keep the Rust workspace and native worker product
  versions aligned, update `CHANGELOG.md`, and tag releases as `vMAJOR.MINOR.PATCH`.
- Treat committed database migrations as immutable. Correct deployed schemas
  with a new forward migration; never edit an existing shared migration.
- CAD originals are read-only during indexing and preview generation. Only the
  explicit managed-move workflow may relocate an original within its current
  library; it must preserve the source-file ID, refuse overwrites, and audit the
  old and new paths.
- Do not commit secrets, credentials, large CAD assets, generated previews,
  build outputs, database contents, or backups.
- Record consequential architecture choices as ADRs in `docs/adr/`.
- Develop VÖLUND natively on Debian. Two deployments are supported (ADR 0039):
  the native systemd installation in `deploy/debian/` and the Docker Compose
  stack in `deploy/docker/`. Both build the same sources from Debian 13 packages
  with the locked dependencies. Keep them functionally equivalent. A change to
  a systemd unit's command, paths, limits, or isolation needs the matching change
  in `deploy/docker/compose.yaml`, and the reverse. Docker must stay optional:
  the native deployment must never depend on it.

See `CONTRIBUTING.md` for the human-facing workflow and release checklist.
