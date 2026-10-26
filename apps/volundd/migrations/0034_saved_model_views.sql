-- Named, per-user model list filters (ADR 0032).
CREATE TABLE volund.saved_model_views (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 80),
    query jsonb NOT NULL CHECK (jsonb_typeof(query) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX saved_model_views_user_name_idx
    ON volund.saved_model_views (user_id, lower(name));

COMMENT ON TABLE volund.saved_model_views IS
    'Bookmarked model search filters of one account; display state, never access policy';
