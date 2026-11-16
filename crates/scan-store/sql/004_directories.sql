-- Folder observations are separate from regular files and never contribute logical bytes.
CREATE TABLE directories (
    scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
    path_key BLOB NOT NULL CHECK (length(path_key) > 0),
    display_path TEXT NOT NULL,
    empty INTEGER CHECK (empty IN (0, 1)),
    PRIMARY KEY (scan_id, path_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX empty_directories ON directories (scan_id, path_key) WHERE empty = 1;

PRAGMA user_version = 4;
