ALTER TABLE volund.import_draft_items
    ADD COLUMN public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    ADD COLUMN upload_status text NOT NULL DEFAULT 'pending'
        CHECK (upload_status IN ('pending', 'uploading', 'uploaded')),
    ADD COLUMN uploaded_bytes bigint NOT NULL DEFAULT 0 CHECK (uploaded_bytes >= 0),
    ADD COLUMN sha256 text CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$'),
    ADD COLUMN upload_started_at timestamptz,
    ADD COLUMN upload_completed_at timestamptz;

CREATE INDEX import_draft_items_upload_status_idx
    ON volund.import_draft_items (import_draft_id, upload_status);

COMMENT ON COLUMN volund.import_draft_items.public_id IS
    'Opaque upload identity; staging filenames never derive from browser paths';
