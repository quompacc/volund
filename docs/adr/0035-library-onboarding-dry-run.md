# ADR 0035: Library onboarding dry run and scan visibility

- Status: Accepted
- Date: 2026-10-26

## Context

Registering a library meant validating that the path exists and is readable,
then registering it and starting a scan. Operators learned what the folder
contained, and whether it could be scanned at all, only after the fact. The
scanner aborts the whole scan if it finds an unreadable directory or a file name
that is not valid UTF-8. A path that overlapped an existing library was
cataloged twice without warning. While a scan ran, library administration
showed only "Scan läuft …": no progress, no cancel button, and no reason when a
scan failed.

## Decision

- `POST /api/v1/libraries/dry-run` (library administrators) validates the path
  as registration does. It then walks it the way the scanner would, skipping
  symlinks and the quarantine folder, but without hashing, writing, or
  registering anything. The report contains:
  - files, bytes, and directories;
  - recognized CAD formats and other files;
  - skipped symlinks;
  - the five largest files;
  - **blockers** that would make the first scan fail, each with its code and
    path (up to 20 listed, all counted);
  - **overlaps** with registered library roots: same path, inside one, or
    containing one.
- The walk is bounded to 500,000 entries and 20 seconds, runs off the async
  executor, and marks a truncated result. A dry run is a preview, not a
  guarantee: the folder can change before the real scan.
- The library list now carries `latestScan`: its ID, status, full or
  incremental, live `discoveredFiles` and `hashedFiles`, attempt, pending
  cancellation, and the sanitized error of a failed scan.
- The administration view adds a "Probelauf" button beside "Pfad prüfen",
  shows progress for queued and running scans, and offers "Scan abbrechen".
  Cancelling uses the existing exact confirmation (`CANCEL <id>`) and job API.
  The cause of a failed scan is shown on the library row.

## Consequences

- Operators see before registering what a library will contain and what would
  break its scan, and they can stop a long scan from where they started it.
- Blockers are reported, not fixed. The scanner keeps failing on them, because
  silently skipping unreadable content would hide missing files.
