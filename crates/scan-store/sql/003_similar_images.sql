ALTER TABLE scans ADD COLUMN similar_images_analyzed INTEGER NOT NULL DEFAULT 0
    CHECK (similar_images_analyzed IN (0, 1) AND (status <> 'cancelled' OR similar_images_analyzed = 0));
ALTER TABLE scans ADD COLUMN similar_group_count INTEGER NOT NULL DEFAULT 0
    CHECK (similar_group_count >= 0);
ALTER TABLE scans ADD COLUMN image_error_count INTEGER NOT NULL DEFAULT 0
    CHECK (image_error_count >= 0);

-- Perceptual similarity is a heuristic, so it stays apart from byte-verified duplicate groups.
CREATE TABLE similar_image_groups (
    scan_id INTEGER NOT NULL REFERENCES scans (id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL CHECK (group_id > 0),
    file_count INTEGER NOT NULL CHECK (file_count > 1),
    PRIMARY KEY (scan_id, group_id)
) STRICT, WITHOUT ROWID;

-- Position 0 is the group's reference image. Distance counts differing hash bits from it.
CREATE TABLE similar_image_members (
    scan_id INTEGER NOT NULL,
    path_key BLOB NOT NULL,
    group_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    difference_hash INTEGER NOT NULL,
    distance INTEGER NOT NULL CHECK (distance BETWEEN 0 AND 64),
    PRIMARY KEY (scan_id, path_key),
    UNIQUE (scan_id, group_id, position),
    FOREIGN KEY (scan_id, path_key) REFERENCES files (scan_id, path_key) ON DELETE CASCADE,
    FOREIGN KEY (scan_id, group_id) REFERENCES similar_image_groups (scan_id, group_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

PRAGMA user_version = 3;
