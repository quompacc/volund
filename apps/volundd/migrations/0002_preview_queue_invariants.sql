CREATE UNIQUE INDEX conversion_runs_one_active_profile_idx
    ON volund.conversion_runs (content_object_id, profile)
    WHERE status IN ('queued', 'running');

COMMENT ON INDEX volund.conversion_runs_one_active_profile_idx IS
    'At most one queued or running conversion per content hash and profile';
