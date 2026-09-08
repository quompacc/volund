# CI quality gates

VÖLUND's committed GitHub Actions workflow runs the public release gate. The
existing Gitea Actions workflow remains available for the private forge. Both
run release-relevant checks in an ephemeral Debian 13 container with an
ephemeral PostgreSQL 17 service. Neither connects to production, mounts CAD
libraries, or consumes deployment credentials.

## Runner contract

The private Gitea runner must register an `ubuntu-latest` label backed by
Docker. GitHub uses a hosted `ubuntu-latest` runner. Both workflows select
`node:20-trixie`; PostgreSQL runs in a separate `postgres:17-bookworm` service
container. Docker remains a CI implementation detail and is not an application
runtime.

The runner needs:

- outbound access to the configured checkout action, Debian mirrors, npm, and
  crates.io;
- enough temporary capacity for Rust, npm, OpenCascade, and CMake outputs;
- no routes, mounts, secrets, or credentials for the production VÖLUND LXC.

## Enforced checks

The native quality job runs:

1. Rust formatting and Clippy with warnings denied;
2. all web tests, the production web build, and npm advisory audit;
3. the native OpenCascade release build and all CTests;
4. all Rust unit, HTTP-contract, Linux, PostgreSQL, job-runner, preview, and
   scanner tests against `volund_ci_test`;
5. RustSec against the complete lockfile with only the reviewed exception in
   [the advisory register](security/advisory-exceptions.md).

After that job succeeds, GitHub creates the source archive twice from the same
commit with normalized gzip metadata, requires byte identity, writes
`SHA256SUMS`, checks that LICENSE is present, and uploads only those two files.
For a tag build, the tag must be exactly `v` plus the common product version.
The workflow does not publish a GitHub Release or deploy to production.

The database name contains `test`, as required by the destructive-test guard.
The PostgreSQL service and all build data disappear with the job.

## Additional native storage-failure gate

The ENOSPC regression is ignored by ordinary `cargo test`: it requires a
private mount namespace and must never fill the host filesystem. Before closing
the storage-failure gate, build `storage_failure_linux` with `cargo test --locked
-p volundd --test storage_failure_linux --no-run`, then run its emitted test
executable using the dedicated harness:

```sh
sudo env VOLUND_TEST_DATABASE_URL="$VOLUND_TEST_DATABASE_URL" \
  sh apps/volundd/tests/storage-volume-linux.sh \
  /absolute/path/to/target/debug/deps/storage_failure_linux-HASH "$(id -un)"
```

Use the exact executable path reported by Cargo and an unprivileged test user.
The database must be disposable. The harness creates a private 4 MiB tmpfs,
runs only the explicitly ignored full-volume test as that user, and unmounts
it on exit. The test verifies tmpfs type and capacity before writing, limits
filling to 8 MiB, and requires real ENOSPC. This additional native gate was
executed during storage-failure acceptance; the standard container job does not
claim mount privileges or run it implicitly.

## Repository policy

Protect `main` on GitHub after the first successful workflow run and require
`quality / debian-native`; apply the equivalent rule in Gitea while it remains
in use. A release tag must point to a commit for which that check succeeded.
Runner registration and branch protection are instance administration tasks
and are intentionally not encoded as repository secrets.
