ALTER TABLE volund.collections
    ADD COLUMN normalized_name text,
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN active boolean NOT NULL DEFAULT true,
    ADD COLUMN removed_at timestamptz,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT collections_name_bounds_check CHECK (
        name = btrim(name) AND length(name) BETWEEN 1 AND 160),
    ADD CONSTRAINT collections_active_shape_check CHECK (active = (removed_at IS NULL)),
    ADD CONSTRAINT collections_updated_check CHECK (updated_at >= created_at);

UPDATE volund.collections
SET name = regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'),
    normalized_name = lower(regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'));

ALTER TABLE volund.collections ALTER COLUMN normalized_name SET NOT NULL;
CREATE UNIQUE INDEX collections_active_normalized_name_idx
    ON volund.collections (normalized_name) WHERE active;
ALTER TABLE volund.collections DROP CONSTRAINT collections_slug_key;
CREATE UNIQUE INDEX collections_active_slug_idx
    ON volund.collections (slug) WHERE active;

ALTER TABLE volund.tags
    ADD COLUMN public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    ADD COLUMN normalized_name text,
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN active boolean NOT NULL DEFAULT true,
    ADD COLUMN merged_into_id bigint REFERENCES volund.tags(id) ON DELETE RESTRICT,
    ADD COLUMN removed_at timestamptz,
    ADD COLUMN created_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT tags_name_bounds_check CHECK (
        name = btrim(name) AND length(name) BETWEEN 1 AND 50),
    ADD CONSTRAINT tags_lifecycle_shape_check CHECK (
        (active AND merged_into_id IS NULL AND removed_at IS NULL) OR
        (NOT active AND ((merged_into_id IS NOT NULL) <> (removed_at IS NOT NULL)))),
    ADD CONSTRAINT tags_not_own_merge_check CHECK (merged_into_id IS NULL OR merged_into_id <> id),
    ADD CONSTRAINT tags_updated_check CHECK (updated_at >= created_at);

UPDATE volund.tags
SET name = regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'),
    normalized_name = lower(regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'));

ALTER TABLE volund.tags ALTER COLUMN normalized_name SET NOT NULL;
CREATE UNIQUE INDEX tags_active_normalized_name_idx
    ON volund.tags (normalized_name) WHERE active;
ALTER TABLE volund.tags DROP CONSTRAINT tags_slug_key;
CREATE UNIQUE INDEX tags_active_slug_idx
    ON volund.tags (slug) WHERE active;
CREATE INDEX tags_merged_into_idx ON volund.tags (merged_into_id)
    WHERE merged_into_id IS NOT NULL;

COMMENT ON COLUMN volund.collections.normalized_name IS
    'Shared NFKC, whitespace-collapsed, lowercase collection identity';
COMMENT ON COLUMN volund.tags.normalized_name IS
    'Shared NFKC, whitespace-collapsed, lowercase tag identity';
COMMENT ON COLUMN volund.tags.merged_into_id IS
    'Retained inactive alias pointing at the surviving tag identity';
