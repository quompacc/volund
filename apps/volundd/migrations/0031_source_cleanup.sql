CREATE TABLE volund.source_cleanup (
    source_file_id bigint PRIMARY KEY REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    obsolete_relative_path text NOT NULL,
    keeper_relative_path text,
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
COMMENT ON TABLE volund.source_cleanup IS
    'Committed source lifecycle unlink intents; retain bytes until catalog commit and retry safely';
