CREATE TABLE volund.authors (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name text NOT NULL CHECK (name <> ''),
    website text,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.models (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(?:-[a-z0-9]+)*$'),
    name text NOT NULL CHECK (name <> ''),
    description text NOT NULL DEFAULT '',
    kind text NOT NULL CHECK (kind IN ('part', 'assembly', 'project')),
    author_id bigint REFERENCES volund.authors(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.model_source_files (
    model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    source_file_id bigint NOT NULL REFERENCES volund.source_files(id) ON DELETE RESTRICT,
    role text NOT NULL CHECK (role IN
        ('master-cad', 'cad', 'printable-mesh', 'document', 'image', 'archive', 'other')),
    is_primary boolean NOT NULL DEFAULT false,
    ordinal integer NOT NULL DEFAULT 0 CHECK (ordinal >= 0),
    PRIMARY KEY (model_id, source_file_id)
);

CREATE UNIQUE INDEX model_source_files_one_primary_idx
    ON volund.model_source_files (model_id) WHERE is_primary;

CREATE TABLE volund.collections (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(?:-[a-z0-9]+)*$'),
    name text NOT NULL CHECK (name <> ''),
    description text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.collection_models (
    collection_id bigint NOT NULL REFERENCES volund.collections(id) ON DELETE CASCADE,
    model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    ordinal integer NOT NULL DEFAULT 0 CHECK (ordinal >= 0),
    PRIMARY KEY (collection_id, model_id)
);

CREATE TABLE volund.tags (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(?:-[a-z0-9]+)*$'),
    name text NOT NULL CHECK (name <> '')
);

CREATE TABLE volund.model_tags (
    model_id bigint NOT NULL REFERENCES volund.models(id) ON DELETE CASCADE,
    tag_id bigint NOT NULL REFERENCES volund.tags(id) ON DELETE CASCADE,
    PRIMARY KEY (model_id, tag_id)
);

CREATE TABLE volund.import_drafts (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    source_name text NOT NULL CHECK (source_name <> ''),
    suggested_model_name text NOT NULL CHECK (suggested_model_name <> ''),
    suggested_slug text NOT NULL CHECK (suggested_slug ~ '^[a-z0-9]+(?:-[a-z0-9]+)*$'),
    status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'committed', 'cancelled')),
    total_files integer NOT NULL CHECK (total_files > 0),
    total_bytes bigint NOT NULL CHECK (total_bytes >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.import_draft_items (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    import_draft_id bigint NOT NULL REFERENCES volund.import_drafts(id) ON DELETE CASCADE,
    original_path text NOT NULL,
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    category text NOT NULL CHECK (category IN
        ('cad', 'mesh', 'document', 'image', 'archive', 'other')),
    suggested_relative_path text NOT NULL,
    is_primary_candidate boolean NOT NULL DEFAULT false,
    UNIQUE (import_draft_id, original_path),
    UNIQUE (import_draft_id, suggested_relative_path)
);

COMMENT ON TABLE volund.model_source_files IS
    'Stable model relationships reference source UUID rows, never mutable paths';
COMMENT ON TABLE volund.import_drafts IS
    'Metadata-only import proposals; no original bytes are written at draft time';
