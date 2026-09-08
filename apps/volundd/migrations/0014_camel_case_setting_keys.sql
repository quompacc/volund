ALTER TABLE volund.instance_settings
    DROP CONSTRAINT instance_settings_setting_key_check;

ALTER TABLE volund.instance_settings
    ADD CONSTRAINT instance_settings_setting_key_check CHECK (
        setting_key ~ '^[a-z][A-Za-z0-9]*(?:[.][a-z][A-Za-z0-9]*)+$'
    );

COMMENT ON CONSTRAINT instance_settings_setting_key_check
    ON volund.instance_settings IS
    'Namespaced code-owned keys; every segment starts lower-case and may use camelCase';
