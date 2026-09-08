CREATE TABLE volund.user_preferences (
    user_id bigint PRIMARY KEY REFERENCES volund.users(id) ON DELETE CASCADE,
    preview_auto_load text NOT NULL DEFAULT 'selected'
        CHECK (preview_auto_load IN ('manual', 'selected', 'visible')),
    background text NOT NULL DEFAULT 'dark'
        CHECK (background IN ('dark', 'light', 'system')),
    grid_visible boolean NOT NULL DEFAULT true,
    contrast text NOT NULL DEFAULT 'balanced'
        CHECK (contrast IN ('balanced', 'high')),
    render_style text NOT NULL DEFAULT 'solid'
        CHECK (render_style IN ('solid', 'wireframe')),
    problem_minimum_severity text NOT NULL DEFAULT 'warning'
        CHECK (problem_minimum_severity IN ('info', 'warning', 'error')),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE volund.user_preferences IS
    'Per-user bounded display and engineering-inspection preferences; never security policy';
