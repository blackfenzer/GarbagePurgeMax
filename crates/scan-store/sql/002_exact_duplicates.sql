ALTER TABLE scans ADD COLUMN duplicates_analyzed INTEGER NOT NULL DEFAULT 0
    CHECK (duplicates_analyzed IN (0, 1) AND (status <> 'cancelled' OR duplicates_analyzed = 0));
ALTER TABLE scans ADD COLUMN duplicate_group_count INTEGER NOT NULL DEFAULT 0
    CHECK (duplicate_group_count >= 0);
ALTER TABLE scans ADD COLUMN duplicate_error_count INTEGER NOT NULL DEFAULT 0
    CHECK (duplicate_error_count >= 0);

CREATE TABLE duplicate_groups (
    scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL CHECK (group_id > 0),
    file_count INTEGER NOT NULL CHECK (file_count > 1),
    hash BLOB NOT NULL CHECK (length(hash) = 32),
    PRIMARY KEY (scan_id, group_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE duplicate_members (
    scan_id INTEGER NOT NULL,
    path_key BLOB NOT NULL,
    group_id INTEGER NOT NULL,
    logical_bytes INTEGER NOT NULL CHECK (logical_bytes >= 0),
    PRIMARY KEY (scan_id, path_key),
    FOREIGN KEY (scan_id, path_key) REFERENCES files (scan_id, path_key) ON DELETE CASCADE,
    FOREIGN KEY (scan_id, group_id) REFERENCES duplicate_groups (scan_id, group_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE INDEX duplicate_members_by_group ON duplicate_members (scan_id, group_id);
CREATE INDEX duplicate_members_by_size ON duplicate_members (scan_id, logical_bytes DESC, path_key);

PRAGMA user_version = 2;
