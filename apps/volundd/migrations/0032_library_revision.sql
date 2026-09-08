ALTER TABLE volund.library_roots
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0);
