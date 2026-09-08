ALTER TABLE volund.import_drafts
    DROP CONSTRAINT import_drafts_target_action_check,
    ADD CONSTRAINT import_drafts_target_action_check
        CHECK (target_action IN ('create', 'update', 'extend'));

COMMENT ON COLUMN volund.import_drafts.target_action IS
    'Explicit create, metadata update, or metadata-preserving additive model extension';

