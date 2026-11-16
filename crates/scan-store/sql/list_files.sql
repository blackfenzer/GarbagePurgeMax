SELECT path_key, display_path, logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM files
WHERE scan_id = :scan_id
  AND (:search = '' OR instr(lower(display_path), lower(:search)) > 0)
  AND (:file_type = '' OR file_type(display_path) = :file_type)
  AND (:modified_before IS NULL OR modified_unix_seconds < :modified_before
       OR (modified_unix_seconds = :modified_before AND modified_subsec_nanos < :modified_before_nanos))
