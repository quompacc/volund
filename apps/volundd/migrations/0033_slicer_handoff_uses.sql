-- Bound and trace the use of short-lived slicer download tokens (ADR 0031).
ALTER TABLE volund.slicer_handoffs
    ADD COLUMN use_count integer NOT NULL DEFAULT 0 CHECK (use_count >= 0),
    ADD COLUMN last_used_at timestamptz,
    ADD CONSTRAINT slicer_handoffs_used_after_creation CHECK (
        last_used_at IS NULL OR last_used_at >= created_at
    );

CREATE INDEX slicer_handoffs_actor_expiry_idx
    ON volund.slicer_handoffs (actor_user_id, expires_at);

COMMENT ON COLUMN volund.slicer_handoffs.use_count IS
    'Downloads served for this token, including range requests; bounded by the daemon';
