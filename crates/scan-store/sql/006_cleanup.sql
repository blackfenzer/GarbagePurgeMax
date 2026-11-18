-- Independent of scan deletion: recovery records must outlive their source index.
CREATE TABLE cleanup_items (
    id INTEGER PRIMARY KEY,
    scan_id INTEGER NOT NULL,
    source_key BLOB NOT NULL CHECK (length(source_key) > 0),
    source_identity BLOB NOT NULL CHECK (length(source_identity) > 0),
    source_proof TEXT NOT NULL,
    keeper_key BLOB,
    keeper_identity BLOB,
    keeper_proof TEXT,
    reason TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'removed', 'moving', 'trashed', 'failed', 'restoring', 'restored', 'recovered', 'attention')),
    receipt_key BLOB,
    receipt_path BLOB,
    message TEXT NOT NULL DEFAULT '',
    created_unix_ms INTEGER NOT NULL,
    updated_unix_ms INTEGER NOT NULL,
    available_before INTEGER CHECK (available_before >= 0),
    available_after INTEGER CHECK (available_after >= 0),
    CHECK ((keeper_key IS NULL) = (keeper_proof IS NULL)),
    CHECK ((keeper_key IS NULL) = (keeper_identity IS NULL)),
    CHECK (keeper_key IS NULL OR keeper_key <> source_key),
    CHECK (keeper_identity IS NULL OR keeper_identity <> source_identity),
    CHECK ((receipt_key IS NULL) = (receipt_path IS NULL)),
    CHECK (state NOT IN ('trashed', 'restoring', 'restored') OR receipt_key IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX cleanup_active_sources ON cleanup_items(source_key)
    WHERE state IN ('queued', 'moving', 'trashed', 'restoring', 'attention');
CREATE INDEX cleanup_history ON cleanup_items(state, id DESC);
CREATE INDEX cleanup_kept_copies ON cleanup_items(keeper_key);
PRAGMA user_version = 6;
