# ADR 0017: Cross-namespace import publication

- Status: Accepted
- Date: 2026-08-28
- Supersedes: The staging-to-library hard-link step in ADR 0016

## Context

The native service grants write access to `/srv/volund/incoming` and
`/srv/volund/library` as separate systemd `ReadWritePaths`. Linux exposes these
as distinct bind mounts inside the service namespace. A hard link between them
fails with `EXDEV` even when both paths refer to the same underlying ext4 data
filesystem.

## Decision

For new files, VÖLUND streams staged bytes to a draft-specific temporary file in
the final library directory, synchronizes it, and hard-links that temporary file
to the final name without overwriting. The temporary link is then removed. The
original staged file remains intact until the database transaction commits.

Relocations entirely within the library continue to use hard-link-first moves.
Rollback removes newly published targets and restores relocated sources in
reverse order. Successful completion removes the isolated draft staging tree.

## Consequences

- Separate least-privilege systemd bind mounts remain possible.
- New bytes are copied once during confirmation instead of linked across mounts.
- A failed confirmation remains retryable without another browser upload.
