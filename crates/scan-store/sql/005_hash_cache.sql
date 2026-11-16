-- One version per native path and algorithm; replacing a file invalidates the version.
CREATE TABLE hash_cache (
    path_key BLOB NOT NULL CHECK (length(path_key) > 0),
    algorithm TEXT NOT NULL,
    version BLOB NOT NULL CHECK (length(version) > 0),
    hash BLOB NOT NULL CHECK (length(hash) = 32),
    last_scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
    PRIMARY KEY (path_key, algorithm)
) STRICT, WITHOUT ROWID;

CREATE INDEX hash_cache_last_scan ON hash_cache (last_scan_id);
ALTER TABLE scans ADD COLUMN hash_cache_hits INTEGER NOT NULL DEFAULT 0 CHECK (hash_cache_hits >= 0);
ALTER TABLE scans ADD COLUMN hash_cache_misses INTEGER NOT NULL DEFAULT 0 CHECK (hash_cache_misses >= 0);

PRAGMA user_version = 5;
