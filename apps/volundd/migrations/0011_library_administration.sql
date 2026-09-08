ALTER TABLE volund.library_roots
    ADD COLUMN enabled boolean NOT NULL DEFAULT true,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN updated_by_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL;

ALTER TABLE volund.library_roots
    ADD CONSTRAINT library_roots_updated_after_created
    CHECK (updated_at >= created_at);

CREATE INDEX library_roots_enabled_key_idx
    ON volund.library_roots (enabled, root_key);
