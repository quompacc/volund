CREATE TABLE volund.users (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    email text NOT NULL CHECK (email = btrim(email) AND length(email) BETWEEN 3 AND 320),
    normalized_email text NOT NULL UNIQUE CHECK (
        normalized_email = lower(btrim(normalized_email)) AND
        normalized_email ~ '^[^[:space:]@]+@[^[:space:]@]+$'
    ),
    display_name text NOT NULL CHECK (
        display_name = btrim(display_name) AND length(display_name) BETWEEN 1 AND 160
    ),
    role text NOT NULL CHECK (role IN ('owner', 'administrator', 'editor', 'viewer')),
    status text NOT NULL DEFAULT 'invited'
        CHECK (status IN ('invited', 'active', 'disabled', 'locked')),
    failed_login_count integer NOT NULL DEFAULT 0 CHECK (failed_login_count >= 0),
    locked_until timestamptz,
    created_by_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_login_at timestamptz,
    CHECK (updated_at >= created_at)
);

CREATE INDEX users_status_role_idx ON volund.users (status, role);

CREATE TABLE volund.password_credentials (
    user_id bigint PRIMARY KEY REFERENCES volund.users(id) ON DELETE CASCADE,
    password_hash text NOT NULL CHECK (
        left(password_hash, 10) = '$argon2id$' AND length(password_hash) <= 512
    ),
    must_change boolean NOT NULL DEFAULT false,
    changed_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.sessions (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE CASCADE,
    token_digest text NOT NULL UNIQUE CHECK (token_digest ~ '^[0-9a-f]{64}$'),
    csrf_digest text NOT NULL CHECK (csrf_digest ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    idle_expires_at timestamptz NOT NULL,
    absolute_expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    revocation_reason text CHECK (
        revocation_reason IS NULL OR length(revocation_reason) BETWEEN 1 AND 160
    ),
    client_address inet,
    user_agent text CHECK (user_agent IS NULL OR length(user_agent) <= 512),
    CHECK (last_seen_at >= created_at),
    CHECK (idle_expires_at > created_at),
    CHECK (absolute_expires_at >= idle_expires_at),
    CHECK ((revoked_at IS NULL) = (revocation_reason IS NULL))
);

CREATE INDEX sessions_user_active_idx
    ON volund.sessions (user_id, absolute_expires_at) WHERE revoked_at IS NULL;

CREATE TABLE volund.instance_state (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    initialized_at timestamptz,
    owner_user_id bigint UNIQUE REFERENCES volund.users(id) ON DELETE RESTRICT,
    CHECK ((initialized_at IS NULL) = (owner_user_id IS NULL))
);

INSERT INTO volund.instance_state (singleton) VALUES (true);

CREATE TABLE volund.instance_settings (
    setting_key text PRIMARY KEY CHECK (
        setting_key ~ '^[a-z][a-z0-9]*(?:[.][a-z][a-z0-9]*)+$'
    ),
    value_json jsonb NOT NULL,
    pending_value_json jsonb,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by_user_id bigint NOT NULL REFERENCES volund.users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (updated_at >= created_at)
);

CREATE TABLE volund.security_audit_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    actor_user_id bigint REFERENCES volund.users(id) ON DELETE SET NULL,
    actor_public_id uuid,
    actor_display_name text CHECK (
        actor_display_name IS NULL OR length(actor_display_name) BETWEEN 1 AND 160
    ),
    action text NOT NULL CHECK (action ~ '^[a-z][a-z0-9_.-]{1,79}$'),
    outcome text NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    target_type text CHECK (target_type IS NULL OR target_type ~ '^[a-z][a-z0-9_.-]{1,79}$'),
    target_public_id uuid,
    request_id uuid,
    client_address inet,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object')
);

CREATE INDEX security_audit_events_occurred_idx
    ON volund.security_audit_events (occurred_at DESC);
CREATE INDEX security_audit_events_actor_idx
    ON volund.security_audit_events (actor_public_id, occurred_at DESC)
    WHERE actor_public_id IS NOT NULL;

COMMENT ON TABLE volund.instance_state IS
    'Singleton first-owner initialization state locked during bootstrap';
COMMENT ON TABLE volund.instance_settings IS
    'Validated non-secret persisted settings; definitions remain code-owned';
COMMENT ON TABLE volund.security_audit_events IS
    'Append-only application security events without submitted secrets';
