ALTER TABLE volund.scan_runs
    DROP CONSTRAINT scan_runs_status_check;

ALTER TABLE volund.scan_runs
    ADD CONSTRAINT scan_runs_status_check
    CHECK (status IN ('queued', 'running', 'completed', 'failed', 'cancelled'));

ALTER TABLE volund.scan_runs
    ADD COLUMN requested_at timestamptz,
    ADD COLUMN full_scan boolean NOT NULL DEFAULT false;

UPDATE volund.scan_runs SET requested_at = started_at;

ALTER TABLE volund.scan_runs
    ALTER COLUMN requested_at SET DEFAULT now(),
    ALTER COLUMN requested_at SET NOT NULL;

CREATE UNIQUE INDEX scan_runs_one_active_per_library_idx
    ON volund.scan_runs (library_root_id)
    WHERE status IN ('queued', 'running');

CREATE INDEX scan_runs_queue_idx
    ON volund.scan_runs (requested_at, id)
    WHERE status = 'queued';
