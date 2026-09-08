ALTER TABLE volund.models
    ADD COLUMN viewer_rotation_x double precision NOT NULL DEFAULT 0,
    ADD COLUMN viewer_rotation_y double precision NOT NULL DEFAULT 0,
    ADD COLUMN viewer_rotation_z double precision NOT NULL DEFAULT 0,
    ADD CONSTRAINT models_viewer_rotation_x_check CHECK (viewer_rotation_x BETWEEN -360 AND 360),
    ADD CONSTRAINT models_viewer_rotation_y_check CHECK (viewer_rotation_y BETWEEN -360 AND 360),
    ADD CONSTRAINT models_viewer_rotation_z_check CHECK (viewer_rotation_z BETWEEN -360 AND 360);

CREATE TABLE volund.model_components (
    parent_model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    child_model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    ordinal integer NOT NULL DEFAULT 0 CHECK (ordinal >= 0),
    PRIMARY KEY (parent_model_id, child_model_id),
    CHECK (parent_model_id <> child_model_id)
);

COMMENT ON COLUMN volund.models.viewer_rotation_x IS
    'User-maintained model presentation rotation in degrees around the viewer X axis';
COMMENT ON TABLE volund.model_components IS
    'Explicit project and assembly containment relationships between logical models';
