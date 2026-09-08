CREATE TABLE volund.operational_log_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    severity text NOT NULL CHECK (severity IN ('debug', 'info', 'warning', 'error')),
    event_name text NOT NULL CHECK (event_name ~ '^[a-z][a-z0-9_.-]{1,79}$'),
    component text NOT NULL CHECK (component ~ '^[a-z][a-z0-9_.-]{1,79}$'),
    code text NOT NULL CHECK (code ~ '^[a-z][a-z0-9_.-]{1,79}$'),
    message text NOT NULL CHECK (length(message) BETWEEN 1 AND 512),
    request_id text CHECK (request_id IS NULL OR length(request_id) BETWEEN 1 AND 64),
    job_id text CHECK (job_id IS NULL OR length(job_id) BETWEEN 1 AND 128),
    run_id text CHECK (run_id IS NULL OR length(run_id) BETWEEN 1 AND 128),
    actor_public_id uuid REFERENCES volund.users(public_id) ON DELETE SET NULL
);

CREATE INDEX operational_log_events_recent_idx
    ON volund.operational_log_events (occurred_at DESC, id DESC);

COMMENT ON TABLE volund.operational_log_events IS
    'Bounded sanitized operational events readable by the unprivileged VÖLUND runtime';
