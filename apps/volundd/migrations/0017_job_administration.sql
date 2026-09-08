ALTER TABLE volund.scan_runs
    ADD COLUMN attempt integer NOT NULL DEFAULT 1 CHECK (attempt > 0),
    ADD COLUMN retry_of_id bigint REFERENCES volund.scan_runs(id) ON DELETE RESTRICT,
    ADD COLUMN cancellation_requested_at timestamptz,
    ADD CONSTRAINT scan_runs_not_own_retry CHECK (retry_of_id IS NULL OR retry_of_id <> id),
    ADD CONSTRAINT scan_runs_cancel_after_request CHECK (
        cancellation_requested_at IS NULL OR cancellation_requested_at >= requested_at
    );

CREATE UNIQUE INDEX scan_runs_one_retry_idx
    ON volund.scan_runs (retry_of_id) WHERE retry_of_id IS NOT NULL;

ALTER TABLE volund.conversion_runs
    ADD COLUMN attempt integer NOT NULL DEFAULT 1 CHECK (attempt > 0),
    ADD COLUMN retry_of_id bigint REFERENCES volund.conversion_runs(id) ON DELETE RESTRICT,
    ADD COLUMN cancellation_requested_at timestamptz,
    ADD CONSTRAINT conversion_runs_not_own_retry CHECK (retry_of_id IS NULL OR retry_of_id <> id),
    ADD CONSTRAINT conversion_runs_cancel_after_request CHECK (
        cancellation_requested_at IS NULL OR cancellation_requested_at >= requested_at
    );

CREATE UNIQUE INDEX conversion_runs_one_retry_idx
    ON volund.conversion_runs (retry_of_id) WHERE retry_of_id IS NOT NULL;
