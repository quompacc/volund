ALTER TABLE volund.authors
    ADD COLUMN normalized_name text,
    ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN provenance_source text NOT NULL DEFAULT 'unknown'
        CHECK (provenance_source IN ('unknown', 'import', 'user', 'website')),
    ADD COLUMN provenance_note text,
    ADD COLUMN active boolean NOT NULL DEFAULT true,
    ADD COLUMN merged_into_id bigint REFERENCES volund.authors(id) ON DELETE RESTRICT,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT authors_name_bounds_check CHECK (
        name = btrim(name) AND length(name) BETWEEN 1 AND 160),
    ADD CONSTRAINT authors_website_check CHECK (
        website IS NULL OR
        (length(website) <= 2048 AND (website LIKE 'https://%' OR website LIKE 'http://%'))),
    ADD CONSTRAINT authors_provenance_note_check CHECK (
        provenance_note IS NULL OR length(provenance_note) BETWEEN 1 AND 2000),
    ADD CONSTRAINT authors_merge_shape_check CHECK (
        (active AND merged_into_id IS NULL) OR (NOT active AND merged_into_id IS NOT NULL)),
    ADD CONSTRAINT authors_not_own_merge_check CHECK (merged_into_id IS NULL OR merged_into_id <> id),
    ADD CONSTRAINT authors_updated_check CHECK (updated_at >= created_at);

UPDATE volund.authors
SET name = regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'),
    normalized_name = lower(regexp_replace(btrim(normalize(name, NFKC)), '[[:space:]]+', ' ', 'g'));

ALTER TABLE volund.authors
    ALTER COLUMN normalized_name SET NOT NULL,
    ADD CONSTRAINT authors_normalized_bounds_check CHECK (
        length(normalized_name) BETWEEN 1 AND 160);

CREATE UNIQUE INDEX authors_active_normalized_name_idx
    ON volund.authors (normalized_name) WHERE active;
CREATE INDEX authors_merged_into_idx
    ON volund.authors (merged_into_id) WHERE merged_into_id IS NOT NULL;

COMMENT ON COLUMN volund.authors.normalized_name IS
    'Application-normalized NFKC, whitespace-collapsed, lowercase author identity';
COMMENT ON COLUMN volund.authors.merged_into_id IS
    'Retained inactive alias pointing at the surviving author identity';

