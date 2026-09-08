ALTER TABLE volund.model_source_files
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN caption text NOT NULL DEFAULT '' CHECK (length(caption) <= 160),
    ADD COLUMN description text NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    ADD COLUMN notes text NOT NULL DEFAULT '' CHECK (length(notes) <= 4000),
    ADD COLUMN printable boolean NOT NULL DEFAULT false,
    ADD COLUMN printed boolean NOT NULL DEFAULT false,
    ADD COLUMN pre_supported boolean NOT NULL DEFAULT false,
    ADD COLUMN up_axis text CHECK (up_axis IN ('x', 'y', 'z')),
    ADD COLUMN support_hint text NOT NULL DEFAULT '' CHECK (length(support_hint) <= 1000),
    ADD COLUMN orientation_x double precision NOT NULL DEFAULT 0 CHECK (orientation_x BETWEEN -360 AND 360),
    ADD COLUMN orientation_y double precision NOT NULL DEFAULT 0 CHECK (orientation_y BETWEEN -360 AND 360),
    ADD COLUMN orientation_z double precision NOT NULL DEFAULT 0 CHECK (orientation_z BETWEEN -360 AND 360),
    ADD COLUMN metadata_updated_at timestamptz NOT NULL DEFAULT now();

COMMENT ON COLUMN volund.model_source_files.revision IS
    'Optimistic revision for metadata on one model/source relationship';
COMMENT ON COLUMN volund.model_source_files.printable IS
    'Explicit operator classification; never permission to execute a slicer';

CREATE TABLE volund.slicer_handoffs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    token_digest text NOT NULL UNIQUE CHECK (token_digest ~ '^[0-9a-f]{64}$'),
    model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    source_file_id bigint NOT NULL REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    actor_user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE RESTRICT,
    target_id text NOT NULL CHECK (target_id ~ '^[a-z0-9][a-z0-9-]{0,31}$'),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX slicer_handoffs_expiry_idx ON volund.slicer_handoffs (expires_at);

COMMENT ON TABLE volund.slicer_handoffs IS
    'Short-lived purpose-bound bearer downloads for explicitly configured desktop slicers';
