-- When each account last opened the in-app notification center (ADR 0038).
CREATE TABLE volund.notification_reads (
    user_id bigint PRIMARY KEY REFERENCES volund.users(id) ON DELETE CASCADE,
    seen_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE volund.notification_reads IS
    'Per-account read marker; notifications themselves are derived from catalog and operations state';
