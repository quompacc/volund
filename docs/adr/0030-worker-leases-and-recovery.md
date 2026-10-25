# ADR 0030: Worker leases and immediate recovery of lost jobs

- Status: Accepted
- Date: 2026-10-25
- Supersedes: the two-hour stale-claim rule of ADR 0007 and ADR 0027

## Context

The preview and scan workers claimed a queued job by setting it to `running`.
Nothing recorded whether the claiming worker was still alive. A claim was
recovered only after it had been `running` for two hours. Workers are killed in
ordinary operation: an out-of-memory converter, a host reboot, or `systemctl
stop`. With the default concurrency of 1, the dead claim counted against the
limit, so one lost worker stopped the whole preview or scan queue for two hours.

A recovered scan was requeued even if its original worker was still running.
Neither completion statement checked that the run was still `running`. A late
worker could therefore complete a run that another worker had already finished.
A scan that killed its worker every time was also requeued forever.

## Decision

A worker claims a job on a dedicated database connection that is detached from
the pool. Inside the claim transaction it takes a session-level advisory lock
`(class, job id)`: class 860756369 for conversions, 860756370 for scans. A
session lock outlives the commit, so no other session ever sees the claimed row
as `running` without its lease. The worker releases the lease by closing that
connection, and only after it has recorded the terminal state. If the worker
dies, the database ends the session and the lease disappears with it.

Before every claim, a worker sweeps the `running` rows. If it can take a row's
lease, no live worker owns that row:

- A lost conversion becomes `failed` with the diagnostic "conversion worker
  stopped before completion", or `cancelled` if cancellation was requested.
  The explicit, audited retry remains the only way to run it again, because a
  conversion that exhausts memory would otherwise be retried in a loop.
- A lost scan is requeued and its `attempt` is incremented. A scan is also
  treated as owned while its per-library scan lock is held, because a direct
  `volundd scan` has no lease. After three lost workers the scan becomes
  `failed`. A pending cancellation always wins. Catalog persistence is one
  transaction, so a lost scan never leaves partial catalog state behind.

Both completions now lock the run and require it to still be `running`. If it
is not, the worker publishes no artifacts, rolls back its catalog changes, and
reports the run as `lost`. When one task in a worker batch fails, its sibling
tasks still finish; previously they were aborted, which orphaned their claims.

### Orderly stop

The workers handle SIGTERM and SIGINT. A stop drops the running job's future,
which kills the converter's process group or abandons the uncommitted scan
transaction. The worker then requeues the job on its still-open lease
connection. A stop is not the job's fault, but a job that keeps running into a
stop, for example the unit's `TimeoutStartSec`, must not loop forever. Each
requeue therefore increments `attempt`, and the third stop fails the job. The
units use `KillMode=mixed` so that SIGTERM reaches only the worker, and
`TimeoutStopSec=30s` as the bound. The preview unit sets `OOMPolicy=continue`:
a converter killed by the memory limit fails its job instead of stopping the
unit and requeueing the same conversion.

### Time limits

The conversion time limit now applies only to the conversion. The runner prints
`state=running` after it obtains the host-wide conversion lock, and the worker
starts its outer bound there. That bound is the limit plus the runner's
30-second TERM-to-KILL grace plus a 5-second margin, so GNU `timeout` can
terminate the converter gracefully and publish the job under `failed/`.
Previously the outer bound was the limit plus one second from spawn. It killed
the runner before its own timeout handling ran, and with
`jobs.conversionConcurrency` above 1 it timed out jobs that were only waiting
for the lock. Those waiting runs still show as `running`.

A timeout now records the existing `timed-out` status instead of `failed`. The
runner reports it with `state=timed-out` and exit status 124. This amends
ADR 0027, which kept timeouts as `failed`.

The runner also accepts the `--linear-deflection` and `--angular-deflection`
options that the worker passes for custom profiles. Before, the runner rejected
them, so every conversion with a custom deflection failed.

No migration is needed. The wall-clock expiry is removed: a long but healthy
conversion is never recovered, and a lost one is recovered by the next timer
tick, normally within seconds.

## Consequences

- Recovery is immediate and exact. It depends on PostgreSQL ending the session
  of a dead client. A crashed host or a network partition ends it once TCP
  keepalive fails; a killed process ends it at once.
- Each running job holds one extra database connection outside the pool. The
  concurrency limits (at most 8) bound this.
- Scans keep their requeue semantics. Conversions keep their manual retry.
- The integration tests kill a real worker and require the next invocation to
  recover its claim without backdating any timestamp. They also cover a live
  claim that must not be stolen, the queue no longer stalling behind a lost
  claim, the scan attempt cap, the direct-scan lock, and late completions.
