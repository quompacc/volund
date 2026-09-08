ALTER TABLE volund.import_drafts
    ADD COLUMN committed_at timestamptz;

COMMENT ON COLUMN volund.import_drafts.committed_at IS
    'Time at which the reviewed staging set was atomically published and registered';
