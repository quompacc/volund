ALTER TABLE volund.import_drafts
    ADD COLUMN target_library_root_id bigint
        REFERENCES volund.library_roots(id) ON DELETE RESTRICT,
    ADD COLUMN target_model_id bigint
        REFERENCES volund.models(id) ON DELETE SET NULL,
    ADD COLUMN planned_base_directory text,
    ADD COLUMN reviewed_at timestamptz;

ALTER TABLE volund.import_draft_items
    ADD COLUMN planned_action text
        CHECK (planned_action IN ('create', 'reuse', 'relocate', 'conflict')),
    ADD COLUMN planned_relative_path text,
    ADD COLUMN matched_source_file_id bigint
        REFERENCES volund.source_files(id) ON DELETE SET NULL;

UPDATE volund.import_draft_items
SET category = 'cad',
    suggested_relative_path = replace(suggested_relative_path, '/Sonstiges/CAD/', '/CAD/')
WHERE lower(original_path) ~ '\.f3d$';

COMMENT ON COLUMN volund.import_draft_items.planned_action IS
    'Persisted review decision; the later commit must revalidate filesystem and hashes';
