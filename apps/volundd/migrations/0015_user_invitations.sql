CREATE TABLE volund.user_invitations (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id bigint NOT NULL UNIQUE REFERENCES volund.users(id) ON DELETE CASCADE,
    token_digest text NOT NULL UNIQUE CHECK (token_digest ~ '^[0-9a-f]{64}$'),
    expires_at timestamptz NOT NULL,
    accepted_at timestamptz,
    created_by_user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (expires_at > created_at),
    CHECK (accepted_at IS NULL OR accepted_at >= created_at)
);

CREATE INDEX user_invitations_expiry_idx
    ON volund.user_invitations (expires_at)
    WHERE accepted_at IS NULL;

COMMENT ON TABLE volund.user_invitations IS
    'Single-use local account activations; only SHA-256 token digests are persisted';
