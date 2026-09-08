CREATE TABLE volund.import_draft_collections (
    import_draft_id bigint NOT NULL REFERENCES volund.import_drafts(id) ON DELETE CASCADE,
    collection_id bigint NOT NULL REFERENCES volund.collections(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (import_draft_id, collection_id),
    UNIQUE (import_draft_id, ordinal)
);

COMMENT ON TABLE volund.import_draft_collections IS
    'User-selected collections retained with an import draft until atomic model persistence';
