ALTER TABLE volund.derived_artifacts
    DROP CONSTRAINT derived_artifacts_artifact_kind_check;

ALTER TABLE volund.derived_artifacts
    ADD CONSTRAINT derived_artifacts_artifact_kind_check CHECK (
        artifact_kind IN (
            'preview-glb',
            'thumbnail-raster',
            'assembly-manifest',
            'diagnostics',
            'result'
        )
    );

COMMENT ON CONSTRAINT derived_artifacts_artifact_kind_check
    ON volund.derived_artifacts IS
    'Closed set of immutable native preview outputs including deterministic raster thumbnails';
