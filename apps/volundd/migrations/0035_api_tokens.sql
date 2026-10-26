-- Personal API tokens for automation (ADR 0037): hashed at rest, scoped,
-- always expiring, shown once.
CREATE TABLE volund.api_tokens (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 80),
    token_prefix text NOT NULL CHECK (token_prefix ~ '^[0-9a-f]{8}$'),
    token_digest text NOT NULL UNIQUE CHECK (token_digest ~ '^[0-9a-f]{64}$'),
    scope text NOT NULL CHECK (scope IN ('read', 'catalog-write')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    last_used_at timestamptz,
    revoked_at timestamptz,
    CHECK (expires_at > created_at AND expires_at <= created_at + interval '366 days'),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX api_tokens_user_idx ON volund.api_tokens (user_id, expires_at);

COMMENT ON TABLE volund.api_tokens IS
    'Bearer tokens for automation; only a SHA-256 digest of the secret is stored';
