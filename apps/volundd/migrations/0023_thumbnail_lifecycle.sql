ALTER TABLE volund.derived_artifacts
    ADD COLUMN public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE;

ALTER TABLE volund.models
    ADD COLUMN thumbnail_kind text NOT NULL DEFAULT 'default',
    ADD COLUMN thumbnail_source_file_id bigint
        REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    ADD COLUMN thumbnail_artifact_id bigint
        REFERENCES volund.derived_artifacts(id) ON DELETE RESTRICT,
    ADD CONSTRAINT models_thumbnail_shape_check CHECK (
        (thumbnail_kind = 'default' AND thumbnail_source_file_id IS NULL AND thumbnail_artifact_id IS NULL) OR
        (thumbnail_kind = 'source-file' AND thumbnail_source_file_id IS NOT NULL AND thumbnail_artifact_id IS NULL) OR
        (thumbnail_kind = 'derived-artifact' AND thumbnail_source_file_id IS NULL AND thumbnail_artifact_id IS NOT NULL)
    );

CREATE INDEX models_thumbnail_source_idx ON volund.models (thumbnail_source_file_id)
    WHERE thumbnail_source_file_id IS NOT NULL;
CREATE INDEX models_thumbnail_artifact_idx ON volund.models (thumbnail_artifact_id)
    WHERE thumbnail_artifact_id IS NOT NULL;

COMMENT ON COLUMN volund.models.thumbnail_kind IS
    'Explicit default, associated raster source, or ready raster artifact selection';
