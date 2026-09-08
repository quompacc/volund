ALTER TABLE volund.models
    ADD COLUMN active boolean NOT NULL DEFAULT true,
    ADD COLUMN removed_at timestamptz,
    ADD CONSTRAINT models_active_shape_check CHECK (active = (removed_at IS NULL));

CREATE INDEX models_active_updated_idx
    ON volund.models (updated_at DESC, id) WHERE active;

ALTER TABLE volund.authors
    DROP CONSTRAINT authors_merge_shape_check,
    ADD COLUMN removed_at timestamptz,
    ADD CONSTRAINT authors_lifecycle_shape_check CHECK (
        (active AND merged_into_id IS NULL AND removed_at IS NULL) OR
        (NOT active AND ((merged_into_id IS NOT NULL) <> (removed_at IS NOT NULL)))
    );

ALTER TABLE volund.source_files
    ADD COLUMN lifecycle_state text NOT NULL DEFAULT 'available'
        CHECK (lifecycle_state IN ('available', 'quarantined', 'purged')),
    ADD COLUMN lifecycle_revision bigint NOT NULL DEFAULT 1 CHECK (lifecycle_revision > 0);

CREATE INDEX source_files_lifecycle_idx
    ON volund.source_files (lifecycle_state, public_id);

CREATE TABLE volund.lifecycle_plans (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    actor_user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE RESTRICT,
    action text NOT NULL CHECK (action IN
        ('model-file.unlink', 'model.remove', 'source.quarantine', 'source.recover',
         'source.purge', 'author.remove', 'tag.remove', 'collection.remove')),
    target_type text NOT NULL CHECK (target_type IN
        ('model-file', 'model', 'source-file', 'author', 'tag', 'collection')),
    target_public_id uuid NOT NULL,
    parent_public_id uuid,
    expected_revision bigint NOT NULL CHECK (expected_revision > 0),
    confirmation text NOT NULL CHECK (length(confirmation) BETWEEN 1 AND 200),
    impact jsonb NOT NULL CHECK (jsonb_typeof(impact) = 'object'),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (expires_at > created_at)
);

CREATE INDEX lifecycle_plans_active_idx
    ON volund.lifecycle_plans (actor_user_id, expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE volund.source_quarantines (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    source_file_id bigint NOT NULL REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    original_relative_path text NOT NULL,
    quarantine_relative_path text NOT NULL UNIQUE,
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    state text NOT NULL CHECK (state IN ('prepared', 'quarantined', 'recovered', 'purged')),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    retention_until timestamptz NOT NULL,
    quarantined_by_user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE RESTRICT,
    recovered_by_user_id bigint REFERENCES volund.users(id) ON DELETE RESTRICT,
    purged_by_user_id bigint REFERENCES volund.users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX source_quarantines_retention_idx
    ON volund.source_quarantines (retention_until, id) WHERE state = 'quarantined';
CREATE UNIQUE INDEX source_quarantines_live_source_idx
    ON volund.source_quarantines (source_file_id)
    WHERE state IN ('prepared', 'quarantined');

COMMENT ON TABLE volund.lifecycle_plans IS
    'Short-lived actor/resource/revision-bound impact previews for destructive catalog actions';
COMMENT ON TABLE volund.source_quarantines IS
    'Verified same-filesystem quarantine intent and recovery/purge evidence';
