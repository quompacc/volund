ALTER TABLE volund.models
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN license_kind text NOT NULL DEFAULT 'not-specified'
        CHECK (license_kind IN ('not-specified', 'spdx', 'custom')),
    ADD COLUMN license_value text,
    ADD CONSTRAINT models_license_shape_check CHECK (
        (license_kind = 'not-specified' AND license_value IS NULL) OR
        (license_kind IN ('spdx', 'custom') AND
            license_value = btrim(license_value) AND
            length(license_value) BETWEEN 1 AND 160)
    );

CREATE INDEX security_audit_events_target_idx
    ON volund.security_audit_events
        (target_type, target_public_id, occurred_at DESC, id DESC)
    WHERE target_public_id IS NOT NULL;

COMMENT ON COLUMN volund.models.revision IS
    'Optimistic-concurrency revision incremented by each catalog mutation';
COMMENT ON COLUMN volund.models.license_kind IS
    'Explicit not-specified, stored SPDX identifier, or user-provided custom license';
COMMENT ON INDEX volund.security_audit_events_target_idx IS
    'Bounded stable model history without whole-object snapshots';

