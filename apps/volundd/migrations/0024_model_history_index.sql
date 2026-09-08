CREATE INDEX security_audit_events_model_history_idx
    ON volund.security_audit_events (target_public_id, occurred_at DESC, id DESC)
    WHERE target_type = 'model' AND target_public_id IS NOT NULL;

COMMENT ON INDEX volund.security_audit_events_model_history_idx IS
    'Bounded model-scoped business history without exposing global security events';
