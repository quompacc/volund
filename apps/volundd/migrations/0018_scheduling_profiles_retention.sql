CREATE TABLE volund.conversion_profiles (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name text NOT NULL UNIQUE CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 80),
    native_preset text NOT NULL CHECK (native_preset IN ('web', 'fine')),
    linear_deflection double precision CHECK (linear_deflection BETWEEN 0.000001 AND 1000),
    angular_deflection double precision CHECK (angular_deflection BETWEEN 0.01 AND 3.141592653589793),
    enabled boolean NOT NULL DEFAULT true,
    built_in boolean NOT NULL DEFAULT false,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (updated_at >= created_at)
);

INSERT INTO volund.conversion_profiles (name, native_preset, built_in)
VALUES ('web', 'web', true), ('fine', 'fine', true);

ALTER TABLE volund.conversion_runs
    ADD COLUMN conversion_profile_id bigint
        REFERENCES volund.conversion_profiles(id) ON DELETE RESTRICT,
    ADD COLUMN conversion_profile_revision bigint CHECK (conversion_profile_revision > 0),
    ADD COLUMN artifact_protected_until timestamptz,
    ADD COLUMN profile_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb
        CHECK (jsonb_typeof(profile_snapshot) = 'object');

UPDATE volund.conversion_runs run
SET conversion_profile_id = profile.id,
    conversion_profile_revision = profile.revision,
    profile_snapshot = jsonb_build_object('nativePreset', run.profile)
FROM volund.conversion_profiles profile
WHERE profile.name = run.profile;

ALTER TABLE volund.conversion_runs ADD CONSTRAINT conversion_runs_profile_snapshot_complete CHECK (
    (conversion_profile_id IS NULL AND conversion_profile_revision IS NULL) OR
    (conversion_profile_id IS NOT NULL AND conversion_profile_revision IS NOT NULL)
);

DROP INDEX volund.conversion_runs_one_active_profile_idx;
CREATE UNIQUE INDEX conversion_runs_one_active_profile_idx
    ON volund.conversion_runs (content_object_id, conversion_profile_id, conversion_profile_revision)
    WHERE status IN ('queued', 'running');

CREATE TABLE volund.scan_schedules (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    library_root_id bigint NOT NULL REFERENCES volund.library_roots(id) ON DELETE RESTRICT,
    name text NOT NULL CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 80),
    local_time time NOT NULL,
    time_zone text NOT NULL CHECK (length(time_zone) BETWEEN 1 AND 64),
    weekday_mask smallint NOT NULL CHECK (weekday_mask BETWEEN 1 AND 127),
    full_scan boolean NOT NULL DEFAULT false,
    enabled boolean NOT NULL DEFAULT true,
    next_run_at timestamptz,
    last_scheduled_at timestamptz,
    last_outcome text CHECK (last_outcome IN ('enqueued', 'coalesced', 'blocked')),
    last_scan_run_id bigint REFERENCES volund.scan_runs(id) ON DELETE SET NULL,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (library_root_id, name),
    CHECK (enabled = (next_run_at IS NOT NULL)),
    CHECK (updated_at >= created_at)
);

ALTER TABLE volund.scan_runs
    ADD COLUMN schedule_id bigint REFERENCES volund.scan_schedules(id) ON DELETE SET NULL,
    ADD COLUMN scheduled_for timestamptz;

CREATE UNIQUE INDEX scan_runs_one_schedule_occurrence_idx
    ON volund.scan_runs (schedule_id, scheduled_for)
    WHERE schedule_id IS NOT NULL;
CREATE INDEX scan_schedules_due_idx ON volund.scan_schedules (next_run_at, id) WHERE enabled;

CREATE TABLE volund.retention_runs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    initiated_by text NOT NULL CHECK (initiated_by IN ('service', 'user')),
    actor_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL,
    status text NOT NULL CHECK (status IN ('completed', 'partial', 'failed')),
    artifact_runs bigint NOT NULL DEFAULT 0 CHECK (artifact_runs >= 0),
    artifact_files bigint NOT NULL DEFAULT 0 CHECK (artifact_files >= 0),
    artifact_bytes bigint NOT NULL DEFAULT 0 CHECK (artifact_bytes >= 0),
    diagnostics_cleared bigint NOT NULL DEFAULT 0 CHECK (diagnostics_cleared >= 0),
    result_codes jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(result_codes) = 'array'),
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz NOT NULL DEFAULT now(),
    CHECK (finished_at >= started_at)
);

ALTER TABLE volund.operational_components DROP CONSTRAINT operational_components_component_key_check;
ALTER TABLE volund.operational_components ADD CONSTRAINT operational_components_component_key_check
    CHECK (component_key IN ('preview-worker', 'scan-worker', 'scheduler', 'retention-worker', 'backup'));
