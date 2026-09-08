CREATE SCHEMA volund;

CREATE TABLE volund.library_roots (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    root_key text NOT NULL UNIQUE
        CHECK (root_key ~ '^[a-z][a-z0-9_-]{0,62}$'),
    display_name text NOT NULL CHECK (display_name <> ''),
    filesystem_path text NOT NULL UNIQUE
        CHECK (left(filesystem_path, 1) = '/'),
    read_only boolean NOT NULL DEFAULT true CHECK (read_only),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.scan_runs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    library_root_id bigint NOT NULL
        REFERENCES volund.library_roots(id) ON DELETE RESTRICT,
    status text NOT NULL
        CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    discovered_files bigint NOT NULL DEFAULT 0 CHECK (discovered_files >= 0),
    hashed_files bigint NOT NULL DEFAULT 0 CHECK (hashed_files >= 0),
    missing_files bigint NOT NULL DEFAULT 0 CHECK (missing_files >= 0),
    error_message text,
    CHECK (finished_at IS NULL OR finished_at >= started_at)
);

CREATE INDEX scan_runs_library_root_started_idx
    ON volund.scan_runs (library_root_id, started_at DESC);

CREATE TABLE volund.content_objects (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    sha256 text NOT NULL UNIQUE
        CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    detected_format text
        CHECK (detected_format IS NULL OR detected_format IN
            ('step', 'iges', 'brep', 'stl', '3mf', 'obj', 'ply', 'gltf', 'glb')),
    media_type text CHECK (media_type IS NULL OR media_type <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE volund.source_files (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    library_root_id bigint NOT NULL
        REFERENCES volund.library_roots(id) ON DELETE RESTRICT,
    content_object_id bigint NOT NULL
        REFERENCES volund.content_objects(id) ON DELETE RESTRICT,
    relative_path text NOT NULL CHECK (
        relative_path <> '' AND
        left(relative_path, 1) <> '/' AND
        right(relative_path, 1) <> '/' AND
        position(E'\\' IN relative_path) = 0 AND
        position('//' IN relative_path) = 0 AND
        relative_path !~ '(^|/)\.{1,2}(/|$)'
    ),
    filesystem_modified_at timestamptz NOT NULL,
    filesystem_device bigint,
    filesystem_inode bigint,
    first_seen_at timestamptz NOT NULL DEFAULT now(),
    last_seen_scan_id bigint NOT NULL
        REFERENCES volund.scan_runs(id) ON DELETE RESTRICT,
    missing_at timestamptz,
    UNIQUE (library_root_id, relative_path)
);

CREATE INDEX source_files_content_object_idx
    ON volund.source_files (content_object_id);
CREATE INDEX source_files_current_idx
    ON volund.source_files (library_root_id, relative_path)
    WHERE missing_at IS NULL;
CREATE INDEX source_files_last_seen_scan_idx
    ON volund.source_files (last_seen_scan_id);

CREATE TABLE volund.file_dependencies (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_file_id bigint NOT NULL
        REFERENCES volund.source_files(id) ON DELETE CASCADE,
    dependency_kind text NOT NULL
        CHECK (dependency_kind IN
            ('buffer', 'material', 'texture', 'external-reference', 'other')),
    raw_reference text NOT NULL CHECK (raw_reference <> ''),
    resolved_source_file_id bigint
        REFERENCES volund.source_files(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (source_file_id, dependency_kind, raw_reference)
);

CREATE INDEX file_dependencies_resolved_source_idx
    ON volund.file_dependencies (resolved_source_file_id)
    WHERE resolved_source_file_id IS NOT NULL;

CREATE TABLE volund.conversion_runs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    public_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    content_object_id bigint NOT NULL
        REFERENCES volund.content_objects(id) ON DELETE RESTRICT,
    converter_name text NOT NULL CHECK (converter_name <> ''),
    converter_version text NOT NULL CHECK (converter_version <> ''),
    contract_version integer NOT NULL CHECK (contract_version > 0),
    profile text NOT NULL CHECK (profile IN ('web', 'fine')),
    status text NOT NULL CHECK (status IN
        ('queued', 'running', 'ready', 'partial', 'failed', 'cancelled', 'timed-out')),
    settings jsonb NOT NULL DEFAULT '{}'::jsonb
        CHECK (jsonb_typeof(settings) = 'object'),
    diagnostics jsonb NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(diagnostics) = 'array'),
    requested_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    CHECK (started_at IS NULL OR started_at >= requested_at),
    CHECK (finished_at IS NULL OR
        (started_at IS NOT NULL AND finished_at >= started_at))
);

CREATE INDEX conversion_runs_content_requested_idx
    ON volund.conversion_runs (content_object_id, requested_at DESC);
CREATE INDEX conversion_runs_pending_idx
    ON volund.conversion_runs (requested_at)
    WHERE status IN ('queued', 'running');

CREATE TABLE volund.derived_artifacts (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversion_run_id bigint NOT NULL
        REFERENCES volund.conversion_runs(id) ON DELETE CASCADE,
    artifact_kind text NOT NULL CHECK (artifact_kind IN
        ('preview-glb', 'assembly-manifest', 'diagnostics', 'result')),
    relative_path text NOT NULL CHECK (
        relative_path <> '' AND
        left(relative_path, 1) <> '/' AND
        position(E'\\' IN relative_path) = 0 AND
        relative_path !~ '(^|/)\.{1,2}(/|$)'
    ),
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    media_type text NOT NULL CHECK (media_type <> ''),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (conversion_run_id, artifact_kind)
);

COMMENT ON SCHEMA volund IS
    'VÖLUND durable metadata; authoritative CAD bytes remain on the filesystem';
