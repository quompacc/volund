CREATE TABLE volund.source_file_moves (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    source_file_id bigint NOT NULL
        REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    previous_relative_path text NOT NULL,
    new_relative_path text NOT NULL,
    moved_at timestamptz NOT NULL DEFAULT now(),
    CHECK (previous_relative_path <> new_relative_path)
);

CREATE INDEX source_file_moves_source_moved_idx
    ON volund.source_file_moves (source_file_id, moved_at DESC);

COMMENT ON TABLE volund.source_file_moves IS
    'Audit trail for explicit, managed moves; source_file public IDs remain stable';
