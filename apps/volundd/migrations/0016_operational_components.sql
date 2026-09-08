CREATE TABLE volund.operational_components (
    component_key text PRIMARY KEY CHECK (component_key IN
        ('preview-worker', 'scan-worker', 'backup')),
    last_outcome text NOT NULL CHECK (last_outcome IN ('success', 'failed')),
    last_succeeded_at timestamptz,
    last_failed_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (last_succeeded_at IS NOT NULL OR last_failed_at IS NOT NULL)
);
