ALTER TABLE volund.import_drafts
    ADD COLUMN model_kind text CHECK (model_kind IN ('part', 'assembly', 'project')),
    ADD COLUMN description text NOT NULL DEFAULT '',
    ADD COLUMN author_name text,
    ADD COLUMN tags text[] NOT NULL DEFAULT '{}',
    ADD COLUMN configured_at timestamptz;

COMMENT ON COLUMN volund.import_drafts.configured_at IS
    'Set after the user has reviewed and saved the model metadata; no file bytes have been transferred yet';
