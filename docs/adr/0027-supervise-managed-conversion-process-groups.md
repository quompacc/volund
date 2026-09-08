# ADR 0027: Supervise managed conversion process groups

- Status: Accepted
- Date: 2026-09-06

## Context

The preview worker launched `volundd convert`, which launched flock, an internal
runner, timeout, and the native converter. Killing only the direct child on
cancellation left the converter and its children running, even though the
database reported the job as cancelled. A persistent Linux regression reproduced
this with a real converter fixture and a separately observed child process.

## Decision

Each managed runner starts in its own Unix process group. A guard owns that
group and sends SIGKILL when execution leaves the supervisor, including when
the Rust future is dropped. Tokio still owns and reaps the direct child.
The guard invokes `/bin/kill` with explicit arguments and the owned group ID;
it does not invoke a shell or search for processes by name.

The private child environment `VOLUND_MANAGED_PROCESS_GROUP=1` tells the
internal runner to use GNU timeout's foreground mode. This keeps converter
descendants in the supervisor's group. Standalone CLI conversion retains the
existing timeout process-group behavior.

The supervisor polls cancellation every 100 ms and bounds the entire runner
operation to the configured timeout plus one second. The outer bound includes
flock wait time and handles a descendant holding output pipes open after its
parent exits. Runner-reported errors and the outer timeout retain the existing
failed-job report and retry contract. Timeout diagnostics distinguish the cause;
this change does not introduce a new database state or migration.

## Consequences and boundaries

Cancellation prevents further converter work and artifact catalog publication.
A successful retry creates the normal linked attempt and artifacts. Original
CAD files remain read-only. A killed run may leave an unreferenced work directory;
the supervisor does not perform broad filesystem cleanup.

The native deployment remains Linux. The converter is a trusted local program;
this group guard is not containment for a process deliberately escaping with
setsid or changing credentials. It is also not a destructor guarantee after
SIGKILL of the supervising worker itself. Service shutdown must terminate the
whole service process tree. The isolated restart test explicitly stops its owned
worker and runner group before starting a new worker executable; it does not
restart an installed service.

Existing two-hour stale-claim recovery remains unchanged. Fresh running claims
are not stolen immediately on restart. Expired conversions become failed or
cancelled and can be explicitly retried; expired scans are requeued or cancelled.
No new heartbeat lease, immediate orphan detection, or systemd deployment is
introduced by this decision.
