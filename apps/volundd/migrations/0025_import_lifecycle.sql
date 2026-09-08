ALTER TABLE volund.import_drafts DROP CONSTRAINT import_drafts_status_check;

ALTER TABLE volund.import_drafts
    ADD COLUMN owner_user_id bigint REFERENCES volund.users(id) ON DELETE RESTRICT,
    ADD COLUMN display_name text,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN expires_at timestamptz NOT NULL DEFAULT (now() + interval '14 days'),
    ADD COLUMN uploaded_bytes bigint NOT NULL DEFAULT 0 CHECK (uploaded_bytes >= 0),
    ADD COLUMN target_action text CHECK (target_action IN ('create', 'update')),
    ADD COLUMN target_model_revision bigint CHECK (target_model_revision IS NULL OR target_model_revision > 0),
    ADD COLUMN license_kind text NOT NULL DEFAULT 'not-specified' CHECK (
        license_kind IN ('not-specified', 'spdx', 'custom')
    ),
    ADD COLUMN license_value text,
    ADD COLUMN primary_item_id bigint REFERENCES volund.import_draft_items(id) ON DELETE SET NULL,
    ADD COLUMN thumbnail_item_id bigint REFERENCES volund.import_draft_items(id) ON DELETE SET NULL,
    ADD COLUMN last_error_code text CHECK (
        last_error_code IS NULL OR last_error_code ~ '^[a-z][a-z0-9_]{1,79}$'
    ),
    ADD COLUMN result_model_public_id uuid,
    ADD COLUMN cancelled_at timestamptz,
    ADD COLUMN expired_at timestamptz,
    ADD COLUMN staging_cleaned_at timestamptz,
    ADD COLUMN commit_started_at timestamptz;

UPDATE volund.import_drafts
SET display_name = left(source_name, 160),
    status = CASE WHEN status = 'draft' THEN 'expired' ELSE status END,
    expired_at = CASE WHEN status = 'draft' THEN now() ELSE NULL END,
    staging_cleaned_at = NULL,
    last_error_code = CASE WHEN status = 'draft' THEN 'legacy_draft_expired' ELSE NULL END,
    updated_at = COALESCE(committed_at, configured_at, created_at),
    expires_at = created_at + interval '14 days',
    target_action = CASE WHEN target_model_id IS NULL THEN 'create' ELSE 'update' END,
    uploaded_bytes = COALESCE((SELECT sum(i.uploaded_bytes) FROM volund.import_draft_items i
        WHERE i.import_draft_id=import_drafts.id),0),
    result_model_public_id = CASE WHEN status='committed' THEN COALESCE(
        (SELECT public_id FROM volund.models WHERE id=target_model_id),
        (SELECT public_id FROM volund.models WHERE slug=suggested_slug)
    ) END;

ALTER TABLE volund.import_drafts
    ALTER COLUMN display_name SET NOT NULL,
    ADD CONSTRAINT import_drafts_status_check CHECK (status IN (
        'draft', 'uploading', 'uploaded', 'review_ready', 'reviewed',
        'committing', 'committed', 'failed', 'cancelled', 'expired'
    )),
    ADD CONSTRAINT import_drafts_owner_check CHECK (
        owner_user_id IS NOT NULL OR status IN ('committed', 'expired')
    ),
    ADD CONSTRAINT import_drafts_terminal_time_check CHECK (
        (status <> 'cancelled' OR cancelled_at IS NOT NULL) AND
        (status <> 'expired' OR expired_at IS NOT NULL) AND
        (status <> 'committed' OR committed_at IS NOT NULL)
    ),
    ADD CONSTRAINT import_drafts_license_check CHECK (
        (license_kind='not-specified' AND license_value IS NULL) OR
        (license_kind IN ('spdx','custom') AND length(license_value) BETWEEN 1 AND 160)
    );

ALTER TABLE volund.import_draft_items
    DROP CONSTRAINT import_draft_items_planned_action_check,
    ADD COLUMN resolution text CHECK (resolution IN (
        'create', 'reuse', 'relocate', 'skip'
    )),
    ADD COLUMN resolution_relative_path text,
    ADD COLUMN archive_expanded_at timestamptz,
    ADD CONSTRAINT import_draft_items_planned_action_check CHECK (
        planned_action IN ('create', 'reuse', 'relocate', 'conflict', 'skip')
    );

CREATE INDEX import_drafts_owner_status_updated_idx
    ON volund.import_drafts (owner_user_id, status, updated_at DESC, id DESC);
CREATE INDEX import_drafts_expiry_idx
    ON volund.import_drafts (expires_at, id)
    WHERE status NOT IN ('committed', 'cancelled', 'expired', 'committing');

CREATE FUNCTION volund.enforce_import_draft_transition() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status = OLD.status OR NEW.status IN ('failed', 'cancelled', 'expired') THEN
        RETURN NEW;
    END IF;
    IF (OLD.status, NEW.status) IN (
        ('draft','uploading'), ('draft','uploaded'),
        ('uploading','uploaded'), ('uploaded','uploading'),
        ('uploaded','review_ready'), ('uploaded','reviewed'),
        ('review_ready','reviewed'), ('reviewed','uploading'),
        ('reviewed','committing'), ('failed','uploading'),
        ('failed','uploaded'), ('failed','reviewed'), ('failed','committing'),
        ('committing','committed')
    ) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'invalid import draft status transition: % -> %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$;

CREATE TRIGGER import_drafts_transition_guard
BEFORE UPDATE OF status ON volund.import_drafts
FOR EACH ROW EXECUTE FUNCTION volund.enforce_import_draft_transition();

ALTER TABLE volund.operational_components
    DROP CONSTRAINT operational_components_component_key_check,
    ADD CONSTRAINT operational_components_component_key_check CHECK (
        component_key IN (
            'preview-worker', 'scan-worker', 'scheduler', 'retention-worker',
            'import-cleanup', 'backup'
        )
    );

CREATE TABLE volund.import_cleanup_runs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz NOT NULL DEFAULT now(),
    status text NOT NULL CHECK (status IN ('completed', 'partial', 'failed')),
    expired_drafts bigint NOT NULL DEFAULT 0 CHECK (expired_drafts >= 0),
    cleaned_drafts bigint NOT NULL DEFAULT 0 CHECK (cleaned_drafts >= 0),
    cleaned_bytes bigint NOT NULL DEFAULT 0 CHECK (cleaned_bytes >= 0),
    result_codes jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(result_codes) = 'array')
);

COMMENT ON COLUMN volund.import_drafts.owner_user_id IS
    'Creating actor; editors access only their own drafts';
COMMENT ON COLUMN volund.import_drafts.uploaded_bytes IS
    'Exact sum of durably published staged item bytes';
COMMENT ON TABLE volund.import_cleanup_runs IS
    'Sanitized durable evidence from the native import cleanup owner';
