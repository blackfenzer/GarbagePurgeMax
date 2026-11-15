CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    root_key BLOB NOT NULL CHECK (length(root_key) > 0),
    display_root TEXT NOT NULL,
    started_unix_ms INTEGER NOT NULL,
    finished_unix_ms INTEGER,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'cancelled')),
    files INTEGER NOT NULL DEFAULT 0 CHECK (files >= 0),
    directories INTEGER NOT NULL DEFAULT 0 CHECK (directories >= 0),
    logical_bytes INTEGER NOT NULL DEFAULT 0 CHECK (logical_bytes >= 0),
    large_files INTEGER NOT NULL DEFAULT 0 CHECK (large_files BETWEEN 0 AND files),
    large_file_threshold INTEGER NOT NULL CHECK (large_file_threshold > 0),
    skipped INTEGER NOT NULL DEFAULT 0 CHECK (skipped >= 0),
    errors INTEGER NOT NULL DEFAULT 0 CHECK (errors >= 0),
    first_error TEXT,
    CHECK ((status = 'running') = (finished_unix_ms IS NULL))
) STRICT;

CREATE INDEX scans_recent ON scans (started_unix_ms DESC, id DESC)
WHERE status <> 'running';

CREATE TABLE files (
    scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
    path_key BLOB NOT NULL CHECK (length(path_key) > 0),
    display_path TEXT NOT NULL,
    logical_bytes INTEGER NOT NULL CHECK (logical_bytes >= 0),
    created_unix_seconds INTEGER,
    created_subsec_nanos INTEGER CHECK (created_subsec_nanos BETWEEN 0 AND 999999999),
    modified_unix_seconds INTEGER,
    modified_subsec_nanos INTEGER CHECK (modified_subsec_nanos BETWEEN 0 AND 999999999),
    read_only INTEGER NOT NULL CHECK (read_only IN (0, 1)),
    PRIMARY KEY (scan_id, path_key),
    CHECK ((created_unix_seconds IS NULL) = (created_subsec_nanos IS NULL)),
    CHECK ((modified_unix_seconds IS NULL) = (modified_subsec_nanos IS NULL))
) STRICT, WITHOUT ROWID;

CREATE INDEX files_by_size ON files (scan_id, logical_bytes DESC, path_key);

PRAGMA user_version = 1;
